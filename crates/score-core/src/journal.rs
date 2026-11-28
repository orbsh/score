//! The run journal: one JSON object per firing, and the reader that turns it
//! back into a seed for a resumed run.

use std::collections::BTreeMap;

use crate::exec::Entry;
use crate::value::{Value, json_string};

pub fn write(graph: u64, entries: &[Entry]) -> String {
    let mut out = format!("{{\"version\":2,\"graph\":\"{graph:016x}\"}}\n");
    for e in entries {
        let kind = match e.kind {
            crate::exec::Kind::Fire => "fire",
            crate::exec::Kind::Budget => "budget",
        };
        out.push_str(&format!(
            "{{\"seq\":{},\"kind\":\"{kind}\",\"node\":{},\"op\":{},\"port\":{},\"value\":{}}}\n",
            e.seq,
            json_string(&e.node),
            json_string(&e.op),
            json_string(&e.port),
            e.value.to_json()
        ));
    }
    out
}

pub struct Journal {
    /// The fingerprint of the graph the entries were produced by, when the
    /// file carries a header.
    pub graph: Option<u64>,
    pub entries: Vec<Entry>,
}

pub fn read(src: &str) -> Result<Journal, String> {
    let mut graph = None;
    let mut entries = Vec::new();
    for (n, line) in src.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut p = Parser {
            b: line.as_bytes(),
            i: 0,
        };
        let obj = p.object().map_err(|e| format!("line {}: {e}", n + 1))?;
        if obj.contains_key("version") {
            match obj.get("graph") {
                Some(Json::Str(hex)) => {
                    graph = Some(
                        u64::from_str_radix(hex, 16)
                            .map_err(|_| format!("line {}: bad `graph`", n + 1))?,
                    );
                }
                None => return Err(format!("line {}: header without `graph`", n + 1)),
                Some(_) => return Err(format!("line {}: bad `graph`", n + 1)),
            }
            continue;
        }
        let seq = match obj.get("seq") {
            Some(Json::Int(i)) if *i >= 0 => *i as usize,
            _ => return Err(format!("line {}: missing `seq`", n + 1)),
        };
        let node = text(&obj, "node", n)?;
        let op = text(&obj, "op", n)?;
        let port = text(&obj, "port", n)?;
        let value = match obj.get("value") {
            Some(j) => to_value(j).ok_or_else(|| format!("line {}: bad value", n + 1))?,
            None => return Err(format!("line {}: missing `value`", n + 1)),
        };
        let kind = match obj.get("kind") {
            Some(Json::Str(k)) if k == "budget" => crate::exec::Kind::Budget,
            Some(Json::Str(k)) if k == "fire" => crate::exec::Kind::Fire,
            None => crate::exec::Kind::Fire,
            Some(_) => return Err(format!("line {}: bad `kind`", n + 1)),
        };
        entries.push(Entry {
            seq,
            node,
            op,
            port,
            value,
            kind,
        });
    }
    Ok(Journal { graph, entries })
}

fn text(obj: &BTreeMap<String, Json>, key: &str, line: usize) -> Result<String, String> {
    match obj.get(key) {
        Some(Json::Str(s)) => Ok(s.clone()),
        _ => Err(format!("line {}: missing `{key}`", line + 1)),
    }
}

fn to_value(j: &Json) -> Option<Value> {
    Some(match j {
        Json::Int(i) => Value::Int(*i),
        Json::Bool(b) => Value::Bool(*b),
        Json::Str(s) => Value::Text(s.clone()),
        Json::Null => Value::Unit,
        Json::Arr(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(to_value(item)?);
            }
            Value::List(out)
        }
        Json::Obj(fields) => {
            if let (Some(Json::Str(tag)), Some(Json::Obj(payload))) =
                (fields.get("variant"), fields.get("fields"))
            {
                let mut out = BTreeMap::new();
                for (k, v) in payload {
                    out.insert(k.clone(), to_value(v)?);
                }
                return Some(Value::Variant {
                    tag: tag.clone(),
                    fields: out,
                });
            }
            let mut out = BTreeMap::new();
            for (k, v) in fields {
                out.insert(k.clone(), to_value(v)?);
            }
            Value::Record(out)
        }
    })
}

#[derive(Debug)]
enum Json {
    Int(i64),
    Bool(bool),
    Null,
    Str(String),
    Arr(Vec<Json>),
    Obj(BTreeMap<String, Json>),
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self
            .b
            .get(self.i)
            .is_some_and(|c| (*c as char).is_ascii_whitespace())
        {
            self.i += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn expect(&mut self, c: u8) -> Result<(), String> {
        self.ws();
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("expected `{}` at byte {}", c as char, self.i))
        }
    }

    fn word(&mut self, w: &[u8]) -> Result<(), String> {
        if self.b[self.i..].starts_with(w) {
            self.i += w.len();
            Ok(())
        } else {
            Err("bad literal".to_string())
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        self.ws();
        match self.peek() {
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b'[') => self.array(),
            Some(b'{') => Ok(Json::Obj(self.object()?)),
            Some(b't') => {
                self.word(b"true")?;
                Ok(Json::Bool(true))
            }
            Some(b'f') => {
                self.word(b"false")?;
                Ok(Json::Bool(false))
            }
            Some(b'n') => {
                self.word(b"null")?;
                Ok(Json::Null)
            }
            Some(_) => self.int(),
            None => Err("unexpected end of line".to_string()),
        }
    }

    fn array(&mut self) -> Result<Json, String> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            items.push(self.value()?);
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(Json::Arr(items));
                }
                _ => return Err("expected `,` or `]`".to_string()),
            }
        }
    }

    fn int(&mut self) -> Result<Json, String> {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
        }
        std::str::from_utf8(&self.b[start..self.i])
            .ok()
            .and_then(|s| s.parse().ok())
            .map(Json::Int)
            .ok_or_else(|| "bad number".to_string())
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let c = self.peek().ok_or("unterminated string")?;
            self.i += 1;
            match c {
                b'"' => return String::from_utf8(out).map_err(|_| "bad utf-8".to_string()),
                b'\\' => {
                    let e = self.peek().ok_or("bad escape")?;
                    self.i += 1;
                    match e {
                        b'"' => out.push(b'"'),
                        b'\\' => out.push(b'\\'),
                        b'/' => out.push(b'/'),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'u' => {
                            let hex = self.b.get(self.i..self.i + 4).ok_or("bad \\u escape")?;
                            let n = u32::from_str_radix(
                                std::str::from_utf8(hex).map_err(|_| "bad \\u escape")?,
                                16,
                            )
                            .map_err(|_| "bad \\u escape")?;
                            self.i += 4;
                            let ch = char::from_u32(n).ok_or("bad codepoint")?;
                            let mut buf = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        _ => return Err("bad escape".to_string()),
                    }
                }
                other => out.push(other),
            }
        }
    }

    fn object(&mut self) -> Result<BTreeMap<String, Json>, String> {
        self.expect(b'{')?;
        let mut map = BTreeMap::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(map);
        }
        loop {
            let key = self.string()?;
            self.expect(b':')?;
            let value = self.value()?;
            map.insert(key, value);
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    return Ok(map);
                }
                _ => return Err("expected `,` or `}`".to_string()),
            }
        }
    }
}

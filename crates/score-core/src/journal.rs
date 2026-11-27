//! The run journal: one JSON object per firing, and the reader that turns it
//! back into a seed for a resumed run.

use std::collections::BTreeMap;

use crate::exec::Entry;
use crate::value::{Value, json_string};

pub fn write(entries: &[Entry]) -> String {
    let mut out = String::new();
    for e in entries {
        out.push_str(&format!(
            "{{\"seq\":{},\"node\":{},\"op\":{},\"port\":{},\"value\":{}}}\n",
            e.seq,
            json_string(&e.node),
            json_string(&e.op),
            json_string(&e.port),
            e.value.to_json()
        ));
    }
    out
}

pub fn read(src: &str) -> Result<Vec<Entry>, String> {
    let mut out = Vec::new();
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
        let seq = match obj.get("seq") {
            Some(Json::Int(i)) if *i >= 0 => *i as usize,
            _ => return Err(format!("line {}: missing `seq`", n + 1)),
        };
        let node = text(&obj, "node", n)?;
        let op = text(&obj, "op", n)?;
        let port = text(&obj, "port", n)?;
        let value = match obj.get("value") {
            Some(Json::Int(i)) => Value::Int(*i),
            Some(Json::Bool(b)) => Value::Bool(*b),
            Some(Json::Str(s)) => Value::Text(s.clone()),
            Some(Json::Null) => Value::Unit,
            None => return Err(format!("line {}: missing `value`", n + 1)),
        };
        out.push(Entry {
            seq,
            node,
            op,
            port,
            value,
        });
    }
    Ok(out)
}

fn text(obj: &BTreeMap<String, Json>, key: &str, line: usize) -> Result<String, String> {
    match obj.get(key) {
        Some(Json::Str(s)) => Ok(s.clone()),
        _ => Err(format!("line {}: missing `{key}`", line + 1)),
    }
}

#[derive(Debug)]
enum Json {
    Int(i64),
    Bool(bool),
    Null,
    Str(String),
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
            Some(b'{') | Some(b'[') => Err("the journal holds scalars only".to_string()),
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

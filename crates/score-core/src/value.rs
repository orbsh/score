//! Values and the small type algebra for the first slice: Int / Text / Bool / Unit.

use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Text(String),
    Bool(bool),
    Unit,
    List(Vec<Value>),
    Record(BTreeMap<String, Value>),
    /// A tagged value: the variant name is the identity, the fields its payload.
    Variant {
        tag: String,
        fields: BTreeMap<String, Value>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ty {
    Int,
    Text,
    Bool,
    Unit,
    List,
    Record,
    Any,
}

impl Ty {
    pub fn name(self) -> &'static str {
        match self {
            Ty::Int => "Int",
            Ty::Text => "Text",
            Ty::Bool => "Bool",
            Ty::Unit => "Unit",
            Ty::List => "List",
            Ty::Record => "Record",
            Ty::Any => "Any",
        }
    }
}

pub fn type_of(v: &Value) -> Ty {
    match v {
        Value::Int(_) => Ty::Int,
        Value::Text(_) => Ty::Text,
        Value::Bool(_) => Ty::Bool,
        Value::Unit => Ty::Unit,
        Value::List(_) => Ty::List,
        Value::Record(_) => Ty::Record,
        Value::Variant { .. } => Ty::Any,
    }
}

pub fn accepts(declared: Ty, actual: Ty) -> bool {
    declared == Ty::Any || actual == Ty::Any || declared == actual
}

impl Value {
    /// The wire form used by the run journal: one JSON value, no schema.
    pub fn to_json(&self) -> String {
        match self {
            Value::Int(i) => i.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Text(s) => json_string(s),
            Value::Unit => "null".to_string(),
            Value::List(items) => {
                let mut out = String::from("[");
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&v.to_json());
                }
                out.push(']');
                out
            }
            Value::Record(fields) => {
                let mut out = String::from("{");
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&json_string(k));
                    out.push(':');
                    out.push_str(&v.to_json());
                }
                out.push('}');
                out
            }
            Value::Variant { tag, fields } => {
                let mut inner = String::from("{");
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        inner.push(',');
                    }
                    inner.push_str(&json_string(k));
                    inner.push(':');
                    inner.push_str(&v.to_json());
                }
                inner.push('}');
                format!("{{\"variant\":{},\"fields\":{inner}}}", json_string(tag))
            }
        }
    }
}

pub fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(i) => write!(f, "{i}"),
            Value::Text(s) => write!(f, "{s}"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Unit => write!(f, "unit"),
            Value::List(items) => {
                write!(f, "[")?;
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{v}")?;
                }
                write!(f, "]")
            }
            Value::Record(fields) => {
                write!(f, "{{")?;
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{k}: {v}")?;
                }
                write!(f, "}}")
            }
            Value::Variant { tag, fields } => {
                write!(f, "{tag}")?;
                if !fields.is_empty() {
                    write!(f, " {{")?;
                    for (i, (k, v)) in fields.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{k}: {v}")?;
                    }
                    write!(f, "}}")?;
                }
                Ok(())
            }
        }
    }
}

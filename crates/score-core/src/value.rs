//! Values and the small type algebra for the first slice: Int / Text / Bool / Unit.

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Text(String),
    Bool(bool),
    Unit,
    List(Vec<Value>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ty {
    Int,
    Text,
    Bool,
    Unit,
    List,
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
        }
    }
}

//! Values and the small type algebra for the first slice: Int / Text / Bool / Unit.

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Text(String),
    Bool(bool),
    Unit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ty {
    Int,
    Text,
    Bool,
    Unit,
    Any,
}

impl Ty {
    pub fn name(self) -> &'static str {
        match self {
            Ty::Int => "Int",
            Ty::Text => "Text",
            Ty::Bool => "Bool",
            Ty::Unit => "Unit",
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
    }
}

pub fn accepts(declared: Ty, actual: Ty) -> bool {
    declared == Ty::Any || actual == Ty::Any || declared == actual
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(i) => write!(f, "{i}"),
            Value::Text(s) => write!(f, "{s}"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Unit => write!(f, "unit"),
        }
    }
}

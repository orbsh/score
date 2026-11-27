//! The operator table for the first slice.

use std::collections::BTreeMap;

use crate::value::{Ty, Value};

pub struct Sig {
    pub ins: &'static [(&'static str, Ty)],
    pub outs: &'static [(&'static str, Ty)],
}

static LIT: Sig = Sig {
    ins: &[("value", Ty::Any)],
    outs: &[("out", Ty::Any)],
};
static ADD: Sig = Sig {
    ins: &[("a", Ty::Int), ("b", Ty::Int)],
    outs: &[("out", Ty::Int)],
};
static CONCAT: Sig = Sig {
    ins: &[("a", Ty::Text), ("b", Ty::Text)],
    outs: &[("out", Ty::Text)],
};
static UPPER: Sig = Sig {
    ins: &[("text", Ty::Text)],
    outs: &[("out", Ty::Text)],
};
static LEN: Sig = Sig {
    ins: &[("text", Ty::Text)],
    outs: &[("out", Ty::Int)],
};
static PRINT: Sig = Sig {
    ins: &[("value", Ty::Any)],
    outs: &[("out", Ty::Unit)],
};

pub fn signature(op: &str) -> Option<&'static Sig> {
    match op {
        "lit" => Some(&LIT),
        "add" => Some(&ADD),
        "concat" => Some(&CONCAT),
        "upper" => Some(&UPPER),
        "len" => Some(&LEN),
        "print" => Some(&PRINT),
        _ => None,
    }
}

pub fn default_in(sig: &Sig) -> Option<&'static str> {
    if sig.ins.len() == 1 {
        return Some(sig.ins[0].0);
    }
    sig.ins
        .iter()
        .find(|(name, _)| *name == "in")
        .map(|(name, _)| *name)
}

pub fn default_out(sig: &Sig) -> Option<&'static str> {
    if sig.outs.len() == 1 {
        return Some(sig.outs[0].0);
    }
    sig.outs
        .iter()
        .find(|(name, _)| *name == "out")
        .map(|(name, _)| *name)
}

pub fn apply(op: &str, args: &BTreeMap<&str, Value>) -> Result<Value, String> {
    let get = |port: &str| {
        args.get(port)
            .cloned()
            .ok_or_else(|| format!("`{op}` is missing its `{port}` input"))
    };
    match op {
        "lit" => Ok(get("value")?),
        "add" => {
            let a = as_int(op, "a", &get("a")?)?;
            let b = as_int(op, "b", &get("b")?)?;
            Ok(Value::Int(a + b))
        }
        "concat" => {
            let av = get("a")?;
            let bv = get("b")?;
            let a = as_text(op, "a", &av)?;
            let b = as_text(op, "b", &bv)?;
            Ok(Value::Text(format!("{a}{b}")))
        }
        "upper" => {
            let v = get("text")?;
            Ok(Value::Text(as_text(op, "text", &v)?.to_uppercase()))
        }
        "len" => {
            let v = get("text")?;
            let text = as_text(op, "text", &v)?;
            Ok(Value::Int(text.chars().count() as i64))
        }
        "print" => {
            let v = get("value")?;
            println!("print: {v}");
            Ok(Value::Unit)
        }
        other => Err(format!("unknown operator `{other}`")),
    }
}

fn as_int(op: &str, port: &str, v: &Value) -> Result<i64, String> {
    match v {
        Value::Int(n) => Ok(*n),
        other => Err(format!("`{op}.{port}` wants Int, got {other:?}")),
    }
}

fn as_text<'a>(op: &str, port: &str, v: &'a Value) -> Result<&'a str, String> {
    match v {
        Value::Text(s) => Ok(s),
        other => Err(format!("`{op}.{port}` wants Text, got {other:?}")),
    }
}

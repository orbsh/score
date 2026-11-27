//! The operator table for the first slice.

use std::collections::BTreeMap;

use crate::value::{Ty, Value, type_of};

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
static RANGE: Sig = Sig {
    ins: &[("n", Ty::Int)],
    outs: &[("out", Ty::List)],
};
static COUNT: Sig = Sig {
    ins: &[("xs", Ty::List)],
    outs: &[("out", Ty::Int)],
};
static SUM: Sig = Sig {
    ins: &[("xs", Ty::List)],
    outs: &[("out", Ty::Int)],
};
static JOIN: Sig = Sig {
    ins: &[("xs", Ty::List), ("sep", Ty::Text)],
    outs: &[("out", Ty::Text)],
};
static AT: Sig = Sig {
    ins: &[("xs", Ty::List), ("index", Ty::Int)],
    outs: &[("out", Ty::Any)],
};

pub fn signature(op: &str) -> Option<&'static Sig> {
    match op {
        "lit" => Some(&LIT),
        "add" => Some(&ADD),
        "concat" => Some(&CONCAT),
        "upper" => Some(&UPPER),
        "len" => Some(&LEN),
        "print" => Some(&PRINT),
        "range" => Some(&RANGE),
        "count" => Some(&COUNT),
        "sum" => Some(&SUM),
        "join" => Some(&JOIN),
        "at" => Some(&AT),
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
        "range" => {
            let n = as_int(op, "n", &get("n")?)?;
            if n < 0 {
                return Err("`range` wants a count that is not negative".to_string());
            }
            Ok(Value::List((0..n).map(Value::Int).collect()))
        }
        "count" => {
            let xs = get("xs")?;
            Ok(Value::Int(as_list(op, "xs", &xs)?.len() as i64))
        }
        "sum" => {
            let xs = get("xs")?;
            let mut total = 0i64;
            for v in as_list(op, "xs", &xs)? {
                total += as_int(op, "xs element", v)?;
            }
            Ok(Value::Int(total))
        }
        "join" => {
            let xs = get("xs")?;
            let sep = get("sep")?;
            let sep = as_text(op, "sep", &sep)?;
            let parts: Vec<String> = as_list(op, "xs", &xs)?
                .iter()
                .map(|v| v.to_string())
                .collect();
            Ok(Value::Text(parts.join(sep)))
        }
        "at" => {
            let xs = get("xs")?;
            let index = as_int(op, "index", &get("index")?)?;
            let items = as_list(op, "xs", &xs)?;
            if index < 0 || index as usize >= items.len() {
                return Err(format!(
                    "`at` index {index} is outside a list of {}",
                    items.len()
                ));
            }
            Ok(items[index as usize].clone())
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

fn as_list<'a>(op: &str, port: &str, v: &'a Value) -> Result<&'a Vec<Value>, String> {
    match v {
        Value::List(items) => Ok(items),
        other => Err(format!(
            "`{op}.{port}` wants List, got {}",
            type_of(other).name()
        )),
    }
}

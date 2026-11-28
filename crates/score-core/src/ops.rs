//! The operator table for the first slice.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use crate::value::{Ty, Value, type_of};

pub struct Sig {
    pub ins: Vec<(&'static str, Ty)>,
    pub outs: Vec<(&'static str, Ty)>,
}

/// The operator table is built at first use instead of being declared as
/// statics: a port type such as `List<Int>` owns its element type, and a const
/// slice cannot hold one.
fn table() -> &'static BTreeMap<&'static str, Sig> {
    static TABLE: OnceLock<BTreeMap<&'static str, Sig>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let entries: Vec<(&'static str, Sig)> = vec![
            (
                "lit",
                Sig {
                    ins: vec![("value", Ty::Any)],
                    outs: vec![("out", Ty::Any)],
                },
            ),
            (
                "add",
                Sig {
                    ins: vec![("a", Ty::Int), ("b", Ty::Int)],
                    outs: vec![("out", Ty::Int)],
                },
            ),
            (
                "concat",
                Sig {
                    ins: vec![("a", Ty::Text), ("b", Ty::Text)],
                    outs: vec![("out", Ty::Text)],
                },
            ),
            (
                "upper",
                Sig {
                    ins: vec![("text", Ty::Text)],
                    outs: vec![("out", Ty::Text)],
                },
            ),
            (
                "len",
                Sig {
                    ins: vec![("text", Ty::Text)],
                    outs: vec![("out", Ty::Int)],
                },
            ),
            (
                "print",
                Sig {
                    ins: vec![("value", Ty::Any)],
                    outs: vec![("out", Ty::Unit)],
                },
            ),
            (
                "range",
                Sig {
                    ins: vec![("n", Ty::Int)],
                    outs: vec![("out", Ty::List)],
                },
            ),
            (
                "count",
                Sig {
                    ins: vec![("xs", Ty::List)],
                    outs: vec![("out", Ty::Int)],
                },
            ),
            (
                "sum",
                Sig {
                    ins: vec![("xs", Ty::List)],
                    outs: vec![("out", Ty::Int)],
                },
            ),
            (
                "join",
                Sig {
                    ins: vec![("xs", Ty::List), ("sep", Ty::Text)],
                    outs: vec![("out", Ty::Text)],
                },
            ),
            (
                "at",
                Sig {
                    ins: vec![("xs", Ty::List), ("index", Ty::Int)],
                    outs: vec![("out", Ty::Any)],
                },
            ),
            (
                "param",
                Sig {
                    ins: vec![],
                    outs: vec![],
                },
            ),
            (
                "map",
                Sig {
                    ins: vec![("f", Ty::Any), ("xs", Ty::List)],
                    outs: vec![("out", Ty::List)],
                },
            ),
            (
                "fold",
                Sig {
                    ins: vec![("f", Ty::Any), ("acc", Ty::Any), ("xs", Ty::List)],
                    outs: vec![("out", Ty::Any)],
                },
            ),
            (
                "filter",
                Sig {
                    ins: vec![("f", Ty::Any), ("xs", Ty::List)],
                    outs: vec![("out", Ty::List)],
                },
            ),
            (
                "any",
                Sig {
                    ins: vec![("f", Ty::Any), ("xs", Ty::List)],
                    outs: vec![("out", Ty::Bool)],
                },
            ),
            (
                "all",
                Sig {
                    ins: vec![("f", Ty::Any), ("xs", Ty::List)],
                    outs: vec![("out", Ty::Bool)],
                },
            ),
            (
                "gt",
                Sig {
                    ins: vec![("a", Ty::Any), ("b", Ty::Any)],
                    outs: vec![("out", Ty::Bool)],
                },
            ),
            (
                "eq",
                Sig {
                    ins: vec![("a", Ty::Any), ("b", Ty::Any)],
                    outs: vec![("out", Ty::Bool)],
                },
            ),
            (
                "record",
                Sig {
                    ins: vec![],
                    outs: vec![("out", Ty::Record)],
                },
            ),
            (
                "get",
                Sig {
                    ins: vec![("r", Ty::Any), ("key", Ty::Text)],
                    outs: vec![("out", Ty::Any)],
                },
            ),
            (
                "tag",
                Sig {
                    ins: vec![("v", Ty::Any)],
                    outs: vec![("out", Ty::Text)],
                },
            ),
        ];
        entries.into_iter().collect()
    })
}

/// An operator that takes its arguments as fields rather than as named ports.
pub fn is_variadic(op: &str) -> bool {
    op == "record"
}

pub fn signature(op: &str) -> Option<&'static Sig> {
    table().get(op)
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
        "gt" => {
            let a = get("a")?;
            let b = get("b")?;
            match (&a, &b) {
                (Value::Int(x), Value::Int(y)) => Ok(Value::Bool(x > y)),
                (Value::Text(x), Value::Text(y)) => Ok(Value::Bool(x > y)),
                _ => Err(format!(
                    "`gt` compares Int or Text, got {} and {}",
                    type_of(&a).name(),
                    type_of(&b).name()
                )),
            }
        }
        "eq" => Ok(Value::Bool(get("a")? == get("b")?)),
        "record" => {
            let mut fields = BTreeMap::new();
            for (name, v) in args {
                fields.insert((*name).to_string(), v.clone());
            }
            Ok(Value::Record(fields))
        }
        "get" => {
            let r = get("r")?;
            let k = get("key")?;
            let key = as_text(op, "key", &k)?;
            match &r {
                Value::Record(fields) | Value::Variant { fields, .. } => fields
                    .get(key)
                    .cloned()
                    .ok_or_else(|| format!("`get`: the record has no field `{key}`")),
                other => Err(format!(
                    "`get.r` wants Record, got {}",
                    type_of(other).name()
                )),
            }
        }
        "map" | "fold" | "filter" | "any" | "all" => Err(format!(
            "`{op}` runs a def per element and is driven by the runtime, not by `apply`"
        )),
        "param" => Err("a parameter is seeded by the runtime, it never fires".to_string()),
        "tag" => {
            let v = get("v")?;
            match &v {
                Value::Variant { tag, .. } => Ok(Value::Text(tag.clone())),
                other => Err(format!(
                    "`tag` wants a variant, got {}",
                    type_of(other).name()
                )),
            }
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

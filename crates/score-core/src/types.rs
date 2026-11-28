//! What a node's output carries.
//!
//! This is a lookup, not inference: every answer is written down somewhere — a
//! port's declared type, a constructor's sum, a struct's fields, a match's arm
//! type. Both the parser (checking a call's arguments) and validation (checking
//! what flows along an edge) ask this same question, so there is one answer.

use std::collections::BTreeMap;

use crate::ast::{Arg, Graph, Node};
use crate::ops;
use crate::value::{Ty, Value};

pub fn field_ty(g: &Graph, by_name: &BTreeMap<&str, &Node>, n: &Node, depth: usize) -> Option<Ty> {
    let src = match n.args.get("r") {
        Some(Arg::Ref(r)) => r,
        _ => return None,
    };
    let key = match n.args.get("key") {
        Some(Arg::Lit(Value::Text(k))) => k,
        _ => return None,
    };
    let Ty::Named(name) = known_ty(g, by_name, by_name.get(src.as_str())?, depth + 1) else {
        return None;
    };
    g.structs
        .get(&name)?
        .iter()
        .find(|(f, _)| f == key)
        .map(|(_, t)| t.clone())
}

/// What a node's output carries, as far as this graph can tell: a port type, a
/// constructor's sum, a struct, or a field read off one of those.
pub fn known_ty(g: &Graph, by_name: &BTreeMap<&str, &Node>, n: &Node, depth: usize) -> Ty {
    if depth > 4 {
        return Ty::Any;
    }
    if let Some(t) = g.node_tys.get(&n.name) {
        return t.clone();
    }
    // A call's type is the def's declared output: the body does not add to it.
    if let Some((_, outs)) = g.sigs.get(&n.op)
        && let Some((_, t)) = outs.iter().find(|(p, _)| *p == n.out_port)
    {
        return t.clone();
    }
    // A field read is checked before the operator table: `get` declares its
    // output as Any, while the field's type is often known exactly.
    if n.op == "get" {
        return field_ty(g, by_name, n, depth).unwrap_or(Ty::Any);
    }
    if let Some(sig) = ops::signature(&n.op)
        && let Some((_, t)) = sig.outs.iter().find(|(p, _)| *p == n.out_port)
    {
        return t.clone();
    }
    if let Some(c) = g.ctors.get(&n.op) {
        return Ty::Named(c.sum.clone());
    }
    if g.structs.contains_key(&n.op) {
        return Ty::Named(n.op.clone());
    }
    Ty::Any
}

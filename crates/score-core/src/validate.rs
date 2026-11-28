//! Validation: ports, types, one edge per input, branch coverage.

use std::collections::{BTreeMap, BTreeSet};

use crate::ast::{Arg, Expr, Graph, Guard, Node};
use crate::ops;
use crate::value::{Ty, accepts, type_of};

pub fn validate(g: &Graph) -> Result<(), String> {
    let mut by_name: BTreeMap<&str, &Node> = BTreeMap::new();
    for n in &g.nodes {
        if by_name.insert(n.name.as_str(), n).is_some() {
            return Err(format!("duplicate node `{}`", n.name));
        }
    }

    // A named type has to exist: the parser cannot tell a typo from a type.
    for (name, (params, outs)) in &g.sigs {
        for (port, ty) in params.iter().chain(outs.iter()) {
            check_named(g, ty, &format!("{name}.{port}"))?;
        }
    }
    for ctor in g.ctors.values() {
        for (field, ty) in &ctor.fields {
            check_named(g, ty, &format!("{}.{field}", ctor.sum))?;
        }
    }
    for (name, fields) in &g.structs {
        for (field, ty) in fields {
            check_named(g, ty, &format!("{name}.{field}"))?;
        }
    }

    for n in &g.nodes {
        if g.ctors.contains_key(&n.op) || g.structs.contains_key(&n.op) {
            // A constructor's arguments were checked by the parser.
            continue;
        }
        if n.op == "match" {
            check_match(g, n, &by_name)?;
            continue;
        }
        let sig = ops::signature(&n.op).ok_or_else(|| format!("unknown operator `{}`", n.op))?;
        if !sig.outs.iter().any(|(p, _)| *p == n.out_port) && n.op != "param" {
            return Err(format!("`{}` has no output port `{}`", n.op, n.out_port));
        }
        for (port, arg) in &n.args {
            if ops::is_variadic(&n.op) {
                if let Arg::Ref(r) = arg
                    && !by_name.contains_key(r.as_str())
                {
                    return Err(format!("unknown node `{r}`"));
                }
                continue;
            }
            let (_, want) = sig
                .ins
                .iter()
                .find(|(p, _)| *p == port.as_str())
                .ok_or_else(|| format!("`{}` has no input port `{}`", n.op, port))?;
            if let Arg::Sub(sub) = arg {
                validate(&sub.graph)?;
                continue;
            }
            let got = match arg {
                Arg::Lit(v) => type_of(v),
                Arg::Sub(_) => Ty::Any,
                Arg::Ref(r) => {
                    let rn = by_name
                        .get(r.as_str())
                        .ok_or_else(|| format!("unknown node `{r}`"))?;
                    if rn.op == "param" {
                        Ty::Any
                    } else if let Some(rsig) = ops::signature(&rn.op) {
                        rsig.outs
                            .iter()
                            .find(|(p, _)| *p == rn.out_port)
                            .map(|(_, t)| t.clone())
                            .ok_or_else(|| {
                                format!("`{}` has no output port `{}`", rn.op, rn.out_port)
                            })?
                    } else {
                        // Constructors and `match` carry no static signature;
                        // their values were checked where they were built.
                        Ty::Any
                    }
                }
            };
            if !accepts(want, &got) {
                return Err(format!(
                    "`{}.{port}` wants {}, got {}",
                    n.name,
                    want.name(),
                    got.name()
                ));
            }
        }
    }

    let mut seen_in: BTreeSet<(String, String)> = BTreeSet::new();
    for e in &g.edges {
        let sn = by_name
            .get(e.from.0.as_str())
            .ok_or_else(|| format!("edge from unknown node `{}`", e.from.0))?;
        let tn = by_name
            .get(e.to.0.as_str())
            .ok_or_else(|| format!("edge to unknown node `{}`", e.to.0))?;
        let tsig = ops::signature(&tn.op);
        let sty = if sn.op == "param" {
            &Ty::Any
        } else if sn.op == "match" {
            g.node_tys.get(&sn.name).unwrap_or(&Ty::Any)
        } else if g.ctors.contains_key(&sn.op) {
            if e.from.1 != sn.op {
                return Err(format!(
                    "`{}` only has the port `{}`, not `{}`",
                    sn.op, sn.op, e.from.1
                ));
            }
            &Ty::Any
        } else if g.structs.contains_key(&sn.op) {
            if e.from.1 != "out" {
                return Err(format!("`{}` only has the port `out`", sn.op));
            }
            &Ty::Record
        } else {
            let ssig =
                ops::signature(&sn.op).ok_or_else(|| format!("unknown operator `{}`", sn.op))?;
            ssig.outs
                .iter()
                .find(|(p, _)| *p == e.from.1)
                .map(|(_, t)| t)
                .ok_or_else(|| format!("`{}` has no output port `{}`", sn.op, e.from.1))?
        };
        if g.ctors.contains_key(&tn.op) || g.structs.contains_key(&tn.op) {
            return Err(format!(
                "`{}` is a constructor; it takes no input edge",
                tn.op
            ));
        }
        let tty = if tn.op == "match" {
            if e.to.1 != "on" {
                return Err(format!("`{}` only takes `on`", tn.name));
            }
            &Ty::Any
        } else {
            let tsig = tsig.ok_or_else(|| format!("unknown operator `{}`", tn.op))?;
            tsig.ins
                .iter()
                .find(|(p, _)| *p == e.to.1)
                .map(|(_, t)| t)
                .ok_or_else(|| format!("`{}` has no input port `{}`", tn.op, e.to.1))?
        };
        if !accepts(tty, sty) {
            return Err(format!(
                "edge `{}.{}` -> `{}.{}` feeds {} into {}",
                e.from.0,
                e.from.1,
                e.to.0,
                e.to.1,
                sty.name(),
                tty.name()
            ));
        }
        if !e.ring && reaches(g, &e.to.0, &e.from.0) {
            return Err(format!(
                "edge `{}.{}` -> `{}.{}` closes a ring; give it a budget: `-[budget: n]->`",
                e.from.0, e.from.1, e.to.0, e.to.1
            ));
        }
        if !e.ring && !seen_in.insert(e.to.clone()) {
            return Err(format!("two edges into `{}.{}`", e.to.0, e.to.1));
        }
        if let Guard::When(expr) = &e.guard
            && let Some(src_sig) = ops::signature(&sn.op)
        {
            check_expr(expr, &e.from.0, src_sig)?;
        }
    }

    for n in &g.nodes {
        if g.ctors.contains_key(&n.op) || g.structs.contains_key(&n.op) || n.op == "match" {
            continue;
        }
        let sig = ops::signature(&n.op).ok_or_else(|| format!("unknown operator `{}`", n.op))?;
        for (port, _) in sig.ins {
            let has_arg = n.args.contains_key(*port);
            let has_edge = g.edges.iter().any(|e| e.to.0 == n.name && e.to.1 == *port);
            if has_arg && has_edge {
                return Err(format!(
                    "input `{}.{port}` has both an argument and an edge",
                    n.name
                ));
            }
            if !has_arg && !has_edge {
                return Err(format!("input `{}.{port}` has no source", n.name));
            }
        }
    }

    let mut from_ports: BTreeMap<(String, String), Vec<&Guard>> = BTreeMap::new();
    for e in &g.edges {
        from_ports.entry(e.from.clone()).or_default().push(&e.guard);
    }
    for (port, guards) in from_ports {
        let always = guards.iter().any(|g| matches!(g, Guard::Always));
        let when = guards.iter().any(|g| matches!(g, Guard::When(_)));
        let fallback = guards.iter().any(|g| matches!(g, Guard::Fallback));
        if when && always {
            return Err(format!(
                "`{}.{}` mixes conditional and unconditional edges",
                port.0, port.1
            ));
        }
        if when && !fallback {
            return Err(format!(
                "branch on `{}.{}` has conditions but no `[*]` arm",
                port.0, port.1
            ));
        }
    }

    Ok(())
}

fn reaches(g: &Graph, from: &str, to: &str) -> bool {
    let mut stack = vec![from];
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    while let Some(node) = stack.pop() {
        if node == to {
            return true;
        }
        if !seen.insert(node) {
            continue;
        }
        for e in &g.edges {
            if e.from.0 == node {
                stack.push(e.to.0.as_str());
            }
        }
    }
    false
}

/// `match` must name every variant of the sum its `on` value carries — that is
/// the part of a sum type worth checking, and it needs no inference: the value
/// comes from a constructor, which names its own sum.
fn check_match(g: &Graph, n: &Node, by_name: &BTreeMap<&str, &Node>) -> Result<(), String> {
    let on = n
        .args
        .get("on")
        .ok_or_else(|| format!("`{}` needs `on:`", n.name))?;
    let src = match on {
        Arg::Ref(r) => by_name
            .get(r.as_str())
            .ok_or_else(|| format!("`{}.on` names unknown node `{r}`", n.name))?,
        Arg::Lit(_) => return Err(format!("`{}.on` must be a node, not a literal", n.name)),
        Arg::Sub(_) => return Err(format!("`{}.on` must be a value", n.name)),
    };
    let ctor = g.ctors.get(&src.op).ok_or_else(|| {
        format!(
            "`{}.on` wants a variant, but `{}` is `{}`",
            n.name, src.name, src.op
        )
    })?;
    let sum = ctor.sum.clone();
    let mut have: BTreeSet<&str> = BTreeSet::new();
    for (k, a) in &n.args {
        if k == "on" {
            continue;
        }
        let Some(arm) = g.ctors.get(k).filter(|c| c.sum == sum) else {
            return Err(format!(
                "`{}` has an arm `{k}`, which is not a variant of `{sum}`",
                n.name
            ));
        };
        if let Arg::Sub(sub) = a {
            let params: Vec<&str> = sub.inputs.iter().map(|(p, _)| p.as_str()).collect();
            let fields: Vec<&str> = arm.fields.iter().map(|(f, _)| f.as_str()).collect();
            if params != fields {
                return Err(format!(
                    "`{}.{k}` takes ({}) but `{k}` carries ({})",
                    n.name,
                    params.join(", "),
                    fields.join(", ")
                ));
            }
        }
        have.insert(k.as_str());
    }
    let missing: Vec<&str> = g
        .ctors
        .iter()
        .filter(|(tag, c)| c.sum == sum && !have.contains(tag.as_str()))
        .map(|(tag, _)| tag.as_str())
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "`{}` on `{sum}` leaves out {}",
            n.name,
            missing.join(", ")
        ));
    }
    Ok(())
}

fn check_named(g: &Graph, ty: &Ty, what: &str) -> Result<(), String> {
    if let Ty::Named(name) = ty
        && !g.structs.contains_key(name)
        && !g.ctors.values().any(|c| c.sum == *name)
    {
        return Err(format!(
            "`{what}` names the type `{name}`, which this program does not declare"
        ));
    }
    Ok(())
}

fn check_expr(e: &Expr, src: &str, sig: &'static ops::Sig) -> Result<(), String> {
    match e {
        Expr::Lit(_) => Ok(()),
        Expr::Local(name) => {
            if sig.outs.iter().any(|(p, _)| p == name) {
                Ok(())
            } else {
                Err(format!("`{name}` is not a port of `{src}`"))
            }
        }
        Expr::Not(x) => check_expr(x, src, sig),
        Expr::Bin(_, a, b) => {
            check_expr(a, src, sig)?;
            check_expr(b, src, sig)
        }
    }
}

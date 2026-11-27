//! Validation: ports, types, one edge per input, branch coverage.

use std::collections::{BTreeMap, BTreeSet};

use crate::ast::{Arg, Expr, Graph, Guard, Node};
use crate::ops;
use crate::value::{accepts, type_of};

pub fn validate(g: &Graph) -> Result<(), String> {
    let mut by_name: BTreeMap<&str, &Node> = BTreeMap::new();
    for n in &g.nodes {
        if by_name.insert(n.name.as_str(), n).is_some() {
            return Err(format!("duplicate node `{}`", n.name));
        }
    }

    for n in &g.nodes {
        let sig = ops::signature(&n.op).ok_or_else(|| format!("unknown operator `{}`", n.op))?;
        if !sig.outs.iter().any(|(p, _)| *p == n.out_port) {
            return Err(format!("`{}` has no output port `{}`", n.op, n.out_port));
        }
        for (port, arg) in &n.args {
            let (_, want) = sig
                .ins
                .iter()
                .find(|(p, _)| *p == port.as_str())
                .ok_or_else(|| format!("`{}` has no input port `{}`", n.op, port))?;
            let got = match arg {
                Arg::Lit(v) => type_of(v),
                Arg::Ref(r) => {
                    let rn = by_name
                        .get(r.as_str())
                        .ok_or_else(|| format!("unknown node `{r}`"))?;
                    let rsig = ops::signature(&rn.op)
                        .ok_or_else(|| format!("unknown operator `{}`", rn.op))?;
                    rsig.outs
                        .iter()
                        .find(|(p, _)| *p == rn.out_port)
                        .map(|(_, t)| *t)
                        .ok_or_else(|| {
                            format!("`{}` has no output port `{}`", rn.op, rn.out_port)
                        })?
                }
            };
            if !accepts(*want, got) {
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
        let ssig = ops::signature(&sn.op).ok_or_else(|| format!("unknown operator `{}`", sn.op))?;
        let tsig = ops::signature(&tn.op).ok_or_else(|| format!("unknown operator `{}`", tn.op))?;
        let (_, sty) = ssig
            .outs
            .iter()
            .find(|(p, _)| *p == e.from.1)
            .ok_or_else(|| format!("`{}` has no output port `{}`", sn.op, e.from.1))?;
        let (_, tty) = tsig
            .ins
            .iter()
            .find(|(p, _)| *p == e.to.1)
            .ok_or_else(|| format!("`{}` has no input port `{}`", tn.op, e.to.1))?;
        if !accepts(*tty, *sty) {
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
        if !seen_in.insert(e.to.clone()) {
            return Err(format!("two edges into `{}.{}`", e.to.0, e.to.1));
        }
        if let Guard::When(expr) = &e.guard {
            check_expr(expr, &e.from.0, ssig)?;
        }
    }

    for n in &g.nodes {
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

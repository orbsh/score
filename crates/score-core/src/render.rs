//! Rendering a graph back to text: the other projection of the same bytes the
//! parser produces, so what a run does can be read without running it.

use std::collections::BTreeMap;
use std::fmt::Write;

use crate::ast::{Arg, Attrs, Edge, Expr, Fields, Graph, Guard, Node};

pub fn graph_text(g: &Graph) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "graph {:016x}", g.fingerprint());

    for (name, (params, outs)) in &g.sigs {
        let params: Vec<String> = params
            .iter()
            .map(|(p, t)| format!("{p}: {}", t.name()))
            .collect();
        let outs: Vec<String> = outs
            .iter()
            .map(|(p, t)| format!("{p}: {}", t.name()))
            .collect();
        let _ = writeln!(
            out,
            "def {name}({}) -> ({}){}",
            params.join(", "),
            outs.join(", "),
            attrs(g.decl_attrs.get(name))
        );
    }
    for (sum, ctors) in sums(g) {
        let arms: Vec<String> = ctors
            .iter()
            .map(|(tag, fields)| {
                if fields.is_empty() {
                    tag.clone()
                } else {
                    let f: Vec<String> = fields
                        .iter()
                        .map(|(n, t)| format!("{n}: {}", t.name()))
                        .collect();
                    format!("{tag} {{ {} }}", f.join(", "))
                }
            })
            .collect();
        let _ = writeln!(
            out,
            "enum {sum} = {}{}",
            arms.join(" | "),
            attrs(g.decl_attrs.get(&sum))
        );
    }
    for (name, fields) in &g.structs {
        let f: Vec<String> = fields
            .iter()
            .map(|(n, t)| format!("{n}: {}", t.name()))
            .collect();
        let _ = writeln!(
            out,
            "struct {name} {{ {} }}{}",
            f.join(", "),
            attrs(g.decl_attrs.get(name))
        );
    }

    let _ = writeln!(out, "nodes");
    for n in &g.nodes {
        let _ = writeln!(
            out,
            "  {}  {}  .{}{}{}{}",
            n.name,
            n.op,
            n.out_port,
            match g.node_tys.get(&n.name) {
                Some(t) => format!(": {}", t.name()),
                None => String::new(),
            },
            args(n),
            attrs(g.node_attrs.get(&n.name))
        );
    }
    let _ = writeln!(out, "edges");
    for e in &g.edges {
        let _ = writeln!(out, "  {}", edge(e));
    }
    out
}

fn sums(g: &Graph) -> BTreeMap<String, Vec<(String, Fields)>> {
    let mut out: BTreeMap<String, Vec<(String, Fields)>> = BTreeMap::new();
    for (tag, ctor) in &g.ctors {
        out.entry(ctor.sum.clone())
            .or_default()
            .push((tag.clone(), ctor.fields.clone()));
    }
    out
}

fn attrs(a: Option<&Attrs>) -> String {
    match a {
        None => String::new(),
        Some(a) if a.is_empty() => String::new(),
        Some(a) => {
            let mut out = String::new();
            for (name, value) in a {
                match value {
                    Some(v) => {
                        let _ = write!(out, " @{name}(\"{v}\")");
                    }
                    None => {
                        let _ = write!(out, " @{name}");
                    }
                }
            }
            out
        }
    }
}

fn args(n: &Node) -> String {
    if n.args.is_empty() {
        return String::new();
    }
    let items: Vec<String> = n
        .args
        .iter()
        .map(|(k, a)| match a {
            Arg::Lit(v) => format!("{k}={v}"),
            Arg::Ref(r) => format!("{k}=@{r}"),
            Arg::Sub(sub) => format!("{k}=<{} of {}>", sub.output.0, sub.graph.nodes.len()),
        })
        .collect();
    format!("  ({})", items.join(", "))
}

fn edge(e: &Edge) -> String {
    let mut out = format!("{}.{} -> {}.{}", e.from.0, e.from.1, e.to.0, e.to.1);
    match &e.guard {
        Guard::Always => {}
        Guard::When(x) => {
            let _ = write!(out, " when {}", expr(x));
        }
        Guard::Fallback => out.push_str(" [*]"),
    }
    if e.ring {
        match e.budget {
            Some(n) => {
                let _ = write!(out, " budget {n}");
            }
            None => out.push_str(" budget unbounded"),
        }
    }
    out
}

fn expr(x: &Expr) -> String {
    match x {
        Expr::Lit(v) => format!("{v}"),
        Expr::Local(n) => n.clone(),
        Expr::Not(inner) => format!("!{}", expr(inner)),
        Expr::Bin(op, l, r) => {
            let sym = match op {
                crate::ast::BinOp::Add => "+",
                crate::ast::BinOp::Sub => "-",
                crate::ast::BinOp::Mul => "*",
                crate::ast::BinOp::Div => "/",
                crate::ast::BinOp::Eq => "==",
                crate::ast::BinOp::Ne => "!=",
                crate::ast::BinOp::Lt => "<",
                crate::ast::BinOp::Le => "<=",
                crate::ast::BinOp::Gt => ">",
                crate::ast::BinOp::Ge => ">=",
                crate::ast::BinOp::And => "&&",
                crate::ast::BinOp::Or => "||",
            };
            format!("{} {sym} {}", expr(l), expr(r))
        }
    }
}

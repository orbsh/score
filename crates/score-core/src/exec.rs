//! Execution: a worklist over the graph. A node fires when every input has a value.

use std::collections::{BTreeMap, BTreeSet};

use crate::ast::{Arg, BinOp, Edge, Expr, Graph, Guard, SubGraph};
use crate::ops;
use crate::value::{Value, type_of};

#[derive(Debug)]
pub struct Run {
    pub values: BTreeMap<(String, String), Value>,
    pub order: Vec<String>,
    /// Nodes that never became ready: an arm a branch did not select, or a dead path.
    pub skipped: Vec<String>,
    /// One entry per firing, in the order the nodes fired.
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub seq: usize,
    pub node: String,
    pub op: String,
    pub port: String,
    pub value: Value,
}

pub fn run(g: &Graph, trace: bool) -> Result<Run, String> {
    run_from(g, trace, &BTreeMap::new(), 0)
}

/// Resume: values already in the journal seed the run, and their edges are
/// re-delivered so downstream nodes see the same inputs they would have seen.
pub fn run_from(
    g: &Graph,
    trace: bool,
    seed: &BTreeMap<(String, String), Value>,
    seq_base: usize,
) -> Result<Run, String> {
    let mut values: BTreeMap<(String, String), Value> = seed.clone();
    let mut done: BTreeSet<String> = BTreeSet::new();
    for (node, port) in seed.keys() {
        let n = g.node(node).ok_or_else(|| {
            format!("the journal has `{node}`, which is not a node of this graph")
        })?;
        if n.out_port != *port {
            return Err(format!(
                "the journal has `{node}.{port}`, but this graph has `{node}.{}`",
                n.out_port
            ));
        }
        done.insert(node.clone());
    }
    for ((node, port), v) in seed {
        deliver(g, node, port, v, &mut values)?;
    }

    let mut order: Vec<String> = Vec::new();
    let mut entries: Vec<Entry> = Vec::new();
    let mut seq = seq_base;

    loop {
        let mut progress = false;
        for node in &g.nodes {
            if done.contains(&node.name) {
                continue;
            }
            let sig = ops::signature(&node.op)
                .ok_or_else(|| format!("unknown operator `{}`", node.op))?;
            let mut inputs: BTreeMap<&str, Value> = BTreeMap::new();
            let mut subs: BTreeMap<&str, &SubGraph> = BTreeMap::new();
            let mut ready = true;
            for (port, _) in sig.ins {
                if let Some(arg) = node.args.get(*port) {
                    match arg {
                        Arg::Sub(sub) => {
                            subs.insert(*port, sub);
                        }
                        Arg::Lit(v) => {
                            inputs.insert(*port, v.clone());
                        }
                        Arg::Ref(r) => {
                            let rn = g.node(r).ok_or_else(|| format!("unknown node `{r}`"))?;
                            match values.get(&(r.clone(), rn.out_port.clone())) {
                                Some(v) => {
                                    inputs.insert(*port, v.clone());
                                }
                                None => {
                                    ready = false;
                                    break;
                                }
                            }
                        }
                    }
                } else if let Some(v) = values.get(&(node.name.clone(), (*port).to_string())) {
                    inputs.insert(*port, v.clone());
                } else {
                    ready = false;
                    break;
                }
            }
            if !ready {
                continue;
            }
            let out = if subs.is_empty() {
                ops::apply(&node.op, &inputs)?
            } else {
                run_container(&node.op, &subs, &inputs)?
            };
            if trace {
                println!("run {} ({}) -> {}", node.name, node.op, out);
            }
            entries.push(Entry {
                seq: seq + 1,
                node: node.name.clone(),
                op: node.op.clone(),
                port: node.out_port.clone(),
                value: out.clone(),
            });
            seq += 1;
            values.insert((node.name.clone(), node.out_port.clone()), out.clone());
            deliver(g, &node.name, &node.out_port, &out, &mut values)?;
            done.insert(node.name.clone());
            order.push(node.name.clone());
            progress = true;
        }
        if !progress {
            break;
        }
    }

    let skipped: Vec<String> = g
        .nodes
        .iter()
        .filter(|n| !done.contains(&n.name))
        .map(|n| n.name.clone())
        .collect();

    Ok(Run {
        values,
        order,
        skipped,
        entries,
    })
}

fn run_container(
    op: &str,
    subs: &BTreeMap<&str, &SubGraph>,
    inputs: &BTreeMap<&str, Value>,
) -> Result<Value, String> {
    let sub = subs
        .get("f")
        .ok_or_else(|| format!("`{op}` needs a def as `f`"))?;
    let xs = match inputs.get("xs") {
        Some(Value::List(items)) => items,
        _ => return Err(format!("`{op}` needs a list in `xs`")),
    };
    match op {
        "map" => {
            let mut out = Vec::with_capacity(xs.len());
            for item in xs {
                let mut seed = BTreeMap::new();
                seed.insert(sub.inputs[0].clone(), item.clone());
                let run = run_from(&sub.graph, false, &seed, 0)?;
                out.push(
                    run.values.get(&sub.output).cloned().ok_or_else(|| {
                        format!("`{op}`: the body produced no `{}`", sub.output.0)
                    })?,
                );
            }
            Ok(Value::List(out))
        }
        "fold" => {
            let mut acc = inputs
                .get("acc")
                .cloned()
                .ok_or_else(|| "`fold` needs `acc`".to_string())?;
            for item in xs {
                let mut seed = BTreeMap::new();
                seed.insert(sub.inputs[0].clone(), acc.clone());
                seed.insert(sub.inputs[1].clone(), item.clone());
                let run = run_from(&sub.graph, false, &seed, 0)?;
                acc =
                    run.values.get(&sub.output).cloned().ok_or_else(|| {
                        format!("`fold`: the body produced no `{}`", sub.output.0)
                    })?;
            }
            Ok(acc)
        }
        other => Err(format!("`{other}` does not run a def per element")),
    }
}

fn deliver(
    g: &Graph,
    src_node: &str,
    src_port: &str,
    v: &Value,
    values: &mut BTreeMap<(String, String), Value>,
) -> Result<(), String> {
    let edges: Vec<&Edge> = g
        .edges
        .iter()
        .filter(|e| e.from.0 == src_node && e.from.1 == src_port)
        .collect();
    if edges.is_empty() {
        return Ok(());
    }
    let exclusive = edges
        .iter()
        .any(|e| matches!(e.guard, Guard::When(_) | Guard::Fallback));
    if !exclusive {
        for e in &edges {
            values.insert(e.to.clone(), v.clone());
        }
        return Ok(());
    }
    for e in &edges {
        if let Guard::When(expr) = &e.guard
            && truthy(&eval(expr, src_node, values)?)?
        {
            values.insert(e.to.clone(), v.clone());
            return Ok(());
        }
    }
    if let Some(e) = edges.iter().find(|e| matches!(e.guard, Guard::Fallback)) {
        values.insert(e.to.clone(), v.clone());
    }
    Ok(())
}

fn eval(e: &Expr, src: &str, values: &BTreeMap<(String, String), Value>) -> Result<Value, String> {
    match e {
        Expr::Lit(v) => Ok(v.clone()),
        Expr::Local(name) => values
            .get(&(src.to_string(), name.clone()))
            .cloned()
            .ok_or_else(|| format!("`{name}` is not a port of `{src}`")),
        Expr::Not(x) => Ok(Value::Bool(!truthy(&eval(x, src, values)?)?)),
        Expr::Bin(op, a, b) => {
            let a = eval(a, src, values)?;
            let b = eval(b, src, values)?;
            bin(*op, &a, &b)
        }
    }
}

fn truthy(v: &Value) -> Result<bool, String> {
    match v {
        Value::Bool(b) => Ok(*b),
        other => Err(format!(
            "a condition must be Bool, got {}",
            type_of(other).name()
        )),
    }
}

fn bin(op: BinOp, a: &Value, b: &Value) -> Result<Value, String> {
    use BinOp::*;
    match (op, a, b) {
        (Add, Value::Int(x), Value::Int(y)) => Ok(Value::Int(x + y)),
        (Sub, Value::Int(x), Value::Int(y)) => Ok(Value::Int(x - y)),
        (Mul, Value::Int(x), Value::Int(y)) => Ok(Value::Int(x * y)),
        (Div, Value::Int(_), Value::Int(0)) => Err("division by zero".to_string()),
        (Div, Value::Int(x), Value::Int(y)) => Ok(Value::Int(x / y)),
        (Add, Value::Text(x), Value::Text(y)) => Ok(Value::Text(format!("{x}{y}"))),
        (Eq, x, y) => Ok(Value::Bool(x == y)),
        (Ne, x, y) => Ok(Value::Bool(x != y)),
        (Lt, Value::Int(x), Value::Int(y)) => Ok(Value::Bool(x < y)),
        (Le, Value::Int(x), Value::Int(y)) => Ok(Value::Bool(x <= y)),
        (Gt, Value::Int(x), Value::Int(y)) => Ok(Value::Bool(x > y)),
        (Ge, Value::Int(x), Value::Int(y)) => Ok(Value::Bool(x >= y)),
        (Lt, Value::Text(x), Value::Text(y)) => Ok(Value::Bool(x < y)),
        (Le, Value::Text(x), Value::Text(y)) => Ok(Value::Bool(x <= y)),
        (Gt, Value::Text(x), Value::Text(y)) => Ok(Value::Bool(x > y)),
        (Ge, Value::Text(x), Value::Text(y)) => Ok(Value::Bool(x >= y)),
        (And, Value::Bool(x), Value::Bool(y)) => Ok(Value::Bool(*x && *y)),
        (Or, Value::Bool(x), Value::Bool(y)) => Ok(Value::Bool(*x || *y)),
        _ => Err(format!(
            "cannot apply {op:?} to {} and {}",
            type_of(a).name(),
            type_of(b).name()
        )),
    }
}

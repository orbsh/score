//! Execution: a worklist over the graph. A node fires when every input has a value.

use std::collections::{BTreeMap, BTreeSet};

use crate::ast::{Arg, BinOp, Edge, Expr, Graph, Guard};
use crate::ops;
use crate::value::{Value, type_of};

pub struct Run {
    pub values: BTreeMap<(String, String), Value>,
    pub order: Vec<String>,
    /// Nodes that never became ready: an arm a branch did not select, or a dead path.
    pub skipped: Vec<String>,
}

pub fn run(g: &Graph, trace: bool) -> Result<Run, String> {
    let mut values: BTreeMap<(String, String), Value> = BTreeMap::new();
    let mut done: BTreeSet<String> = BTreeSet::new();
    let mut order: Vec<String> = Vec::new();

    loop {
        let mut progress = false;
        for node in &g.nodes {
            if done.contains(&node.name) {
                continue;
            }
            let sig = ops::signature(&node.op)
                .ok_or_else(|| format!("unknown operator `{}`", node.op))?;
            let mut inputs: BTreeMap<&str, Value> = BTreeMap::new();
            let mut ready = true;
            for (port, _) in sig.ins {
                if let Some(arg) = node.args.get(*port) {
                    match arg {
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
            let out = ops::apply(&node.op, &inputs)?;
            if trace {
                println!("run {} ({}) -> {}", node.name, node.op, out);
            }
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
    })
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

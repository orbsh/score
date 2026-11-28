//! Execution: a worklist over the graph. A node fires when every input has a value.

use std::collections::{BTreeMap, BTreeSet};

use crate::ast::{Arg, BinOp, Body, Expr, Graph, Guard, Node, SubGraph};
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
    pub kind: Kind,
}

/// A journal line is either a firing or the budget left on a back edge. The
/// second kind exists because a ring's progress is not derivable from the
/// firings: resuming without it would hand the ring its budget twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Fire,
    Budget,
}

pub fn run(g: &Graph, trace: bool) -> Result<Run, String> {
    run_from(g, trace, &BTreeMap::new(), &BTreeMap::new(), 0)
}

/// Where a frame hands its results: the call node in the parent frame, and each
/// declared output with the slot that carries it.
type Caller = Option<(String, Vec<(String, (String, String))>)>;

/// One frame per running graph: the top-level program, or a def body being run
/// for a call. State that used to live per run lives per frame now, because the
/// same body can be running several times at once (recursion).
struct Frame<'a> {
    g: &'a Graph,
    values: BTreeMap<(String, String), Value>,
    stamps: BTreeMap<(String, String), u64>,
    budgets: Vec<Option<u32>>,
    fired: BTreeMap<String, Vec<u64>>,
    seeded: BTreeSet<String>,
    /// A call this frame fired and is waiting on: its node name and the marks
    /// it fired with. The call is neither done nor re-fireable until then.
    pending: Option<(String, Vec<u64>)>,
    /// Where the current pass has got to. A pass fires nodes in order, so a
    /// node fired early does not jump ahead of the ones after it in the same
    /// round — that is what keeps a ring's downstream nodes seeing each turn.
    cursor: usize,
    fired_in_pass: bool,
    caller: Caller,
    order: Vec<String>,
}

impl<'a> Frame<'a> {
    fn new(
        g: &'a Graph,
        seed: &BTreeMap<(String, String), Value>,
        budgets_seed: &BTreeMap<(String, String), u32>,
        caller: Caller,
    ) -> Self {
        let mut values: BTreeMap<(String, String), Value> = seed.clone();
        let mut stamps: BTreeMap<(String, String), u64> = BTreeMap::new();
        let mut seeded: BTreeSet<String> = BTreeSet::new();
        for (node, port) in seed.keys() {
            stamps.insert((node.clone(), port.clone()), 1);
            seeded.insert(node.clone());
        }
        let budgets = g
            .edges
            .iter()
            .map(|e| {
                if e.ring {
                    budgets_seed.get(&e.to).copied().or(e.budget)
                } else {
                    None
                }
            })
            .collect();
        values.retain(|_, _| true);
        Frame {
            g,
            values,
            stamps,
            budgets,
            fired: BTreeMap::new(),
            seeded,
            pending: None,
            cursor: 0,
            fired_in_pass: false,
            caller,
            order: Vec::new(),
        }
    }
}

enum Action<'a> {
    Fire {
        name: String,
        op: String,
        slot: (String, String),
        out: Value,
        marks: Vec<u64>,
    },
    Call {
        name: String,
        op: String,
        marks: Vec<u64>,
        inputs: BTreeMap<String, Value>,
        body: &'a Body,
    },
}

/// The input ports of a node: an operator's declared ones, a def's parameters,
/// or — for a constructor, `match`, or a variadic operator — whatever its
/// arguments and edges name.
fn ins_of(g: &Graph, op: &str) -> Option<Vec<String>> {
    // A variadic operator takes its arguments as fields, so its ports are
    // whatever the node names.
    if ops::is_variadic(op) {
        return None;
    }
    if let Some(sig) = ops::signature(op) {
        return Some(sig.ins.iter().map(|(p, _)| (*p).to_string()).collect());
    }
    g.sigs
        .get(op)
        .map(|(ins, _)| ins.iter().map(|(p, _)| p.clone()).collect())
}

fn next_action<'a>(
    f: &Frame<'a>,
    root: &'a Graph,
    from: usize,
) -> Result<Option<(usize, Action<'a>)>, String> {
    for (i, node) in f.g.nodes.iter().enumerate().skip(from) {
        if f.seeded.contains(&node.name) {
            continue;
        }
        if let Some((pending, _)) = &f.pending
            && *pending == node.name
        {
            continue;
        }
        if ops::signature(&node.op).is_none()
            && !f.g.ctors.contains_key(&node.op)
            && !f.g.structs.contains_key(&node.op)
            && !f.g.sigs.contains_key(&node.op)
            && node.op != "match"
        {
            return Err(format!("unknown operator `{}`", node.op));
        }
        let ports: Vec<String> =
            ins_of(f.g, &node.op).unwrap_or_else(|| node.args.keys().cloned().collect());
        let mut inputs: BTreeMap<String, Value> = BTreeMap::new();
        let mut subs: BTreeMap<String, SubGraph> = BTreeMap::new();
        let mut marks: Vec<u64> = Vec::new();
        let mut ready = true;
        for port in &ports {
            match node.args.get(port) {
                Some(Arg::Sub(sub)) => {
                    subs.insert(port.clone(), (**sub).clone());
                    marks.push(0);
                }
                Some(Arg::Lit(v)) => {
                    inputs.insert(port.clone(), v.clone());
                    marks.push(0);
                }
                Some(Arg::Ref(r)) => {
                    let rn = f.g.node(r).ok_or_else(|| format!("unknown node `{r}`"))?;
                    let key = (r.clone(), rn.out_port.clone());
                    match f.values.get(&key) {
                        Some(v) => {
                            marks.push(f.stamps.get(&key).copied().unwrap_or(0));
                            inputs.insert(port.clone(), v.clone());
                        }
                        None => {
                            ready = false;
                            break;
                        }
                    }
                }
                None => {
                    let key = (node.name.clone(), port.clone());
                    match f.values.get(&key) {
                        Some(v) => {
                            marks.push(f.stamps.get(&key).copied().unwrap_or(0));
                            inputs.insert(port.clone(), v.clone());
                        }
                        None => {
                            ready = false;
                            break;
                        }
                    }
                }
            }
        }
        if !ready || f.fired.get(&node.name) == Some(&marks) {
            continue;
        }
        if let Some(body) = root.defs.get(&node.op) {
            return Ok(Some((
                i,
                Action::Call {
                    name: node.name.clone(),
                    op: node.op.clone(),
                    marks,
                    inputs,
                    body,
                },
            )));
        }
        let out = eval_node(f.g, node, &inputs, &subs)?;
        let slot = match &out {
            Value::Variant { tag, .. } => (node.name.clone(), tag.clone()),
            _ => (node.name.clone(), node.out_port.clone()),
        };
        return Ok(Some((
            i,
            Action::Fire {
                name: node.name.clone(),
                op: node.op.clone(),
                slot,
                out,
                marks,
            },
        )));
    }
    Ok(None)
}

fn eval_node(
    g: &Graph,
    node: &Node,
    inputs: &BTreeMap<String, Value>,
    subs: &BTreeMap<String, SubGraph>,
) -> Result<Value, String> {
    if !subs.is_empty() && node.op == "match" {
        let borrowed: BTreeMap<&str, &SubGraph> =
            subs.iter().map(|(k, v)| (k.as_str(), v)).collect();
        let borrowed_inputs: BTreeMap<&str, Value> = inputs
            .iter()
            .map(|(k, v)| (k.as_str(), v.clone()))
            .collect();
        return run_match(&borrowed, &borrowed_inputs);
    }
    if !subs.is_empty() {
        let borrowed: BTreeMap<&str, &SubGraph> =
            subs.iter().map(|(k, v)| (k.as_str(), v)).collect();
        let borrowed_inputs: BTreeMap<&str, Value> = inputs
            .iter()
            .map(|(k, v)| (k.as_str(), v.clone()))
            .collect();
        return run_container(&node.op, &borrowed, &borrowed_inputs);
    }
    if g.ctors.contains_key(&node.op) {
        return Ok(Value::Variant {
            tag: node.op.clone(),
            fields: inputs.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        });
    }
    if g.structs.contains_key(&node.op) {
        return Ok(Value::Record(
            inputs.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        ));
    }
    let borrowed: BTreeMap<&str, Value> = inputs
        .iter()
        .map(|(k, v)| (k.as_str(), v.clone()))
        .collect();
    ops::apply(&node.op, &borrowed)
}

/// Resume: values already in the journal seed the run, and their edges are
/// re-delivered so downstream nodes see the same inputs they would have seen.
pub fn run_from(
    g: &Graph,
    trace: bool,
    seed: &BTreeMap<(String, String), Value>,
    budgets_seed: &BTreeMap<(String, String), u32>,
    seq_base: usize,
) -> Result<Run, String> {
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
    }

    let mut stack: Vec<Frame> = vec![Frame::new(g, seed, budgets_seed, None)];
    let mut entries: Vec<Entry> = Vec::new();
    let mut seq = seq_base;
    for (slot, v) in seed {
        let spent = {
            let f = stack.last_mut().unwrap();
            deliver(f.g, slot, v, &mut f.values, &mut f.stamps, &mut f.budgets)?
        };
        for ((node, port), left) in spent {
            seq += 1;
            entries.push(Entry {
                seq,
                node,
                op: "@budget".to_string(),
                port,
                value: Value::Int(left as i64),
                kind: Kind::Budget,
            });
        }
    }

    let mut fires = 0usize;
    loop {
        let idx = stack.len() - 1;
        let found = {
            let f = &stack[idx];
            next_action(f, g, f.cursor)?
        };
        let Some((at, action)) = found else {
            if !stack[idx].fired_in_pass {
                if idx == 0 {
                    break;
                }
                // The frame has converged: hand each declared output to the
                // call node that is waiting in the parent.
                let child = stack.pop().expect("a child frame");
                let (call_node, outs) = child
                    .caller
                    .clone()
                    .expect("a frame below the root has a caller");
                let parent = stack.last_mut().expect("the parent frame");
                let pending = parent
                    .pending
                    .take()
                    .ok_or_else(|| format!("`{call_node}` returned to no caller"))?;
                let mut landed: Vec<(String, Value)> = Vec::new();
                for (out_name, (node, port)) in &outs {
                    let v = child
                        .values
                        .get(&(node.clone(), port.clone()))
                        .cloned()
                        .or_else(|| {
                            child
                                .values
                                .iter()
                                .find(|((n, _), _)| n == node)
                                .map(|(_, v)| v.clone())
                        })
                        .ok_or_else(|| {
                            format!("`{call_node}`: the body produced no `{out_name}`")
                        })?;
                    landed.push((out_name.clone(), v));
                }
                parent.fired.insert(call_node.clone(), pending.1);
                parent.order.push(call_node.clone());
                for (out_name, v) in landed {
                    let slot = match &v {
                        Value::Variant { tag, .. } => (call_node.clone(), tag.clone()),
                        _ => (call_node.clone(), out_name),
                    };
                    seq += 1;
                    entries.push(Entry {
                        seq,
                        node: call_node.clone(),
                        op: "call".to_string(),
                        port: slot.1.clone(),
                        value: v.clone(),
                        kind: Kind::Fire,
                    });
                    parent.values.insert(slot.clone(), v.clone());
                    *parent.stamps.entry(slot.clone()).or_insert(0) += 1;
                    let spent = deliver(
                        parent.g,
                        &slot,
                        &v,
                        &mut parent.values,
                        &mut parent.stamps,
                        &mut parent.budgets,
                    )?;
                    for ((node, port), left) in spent {
                        seq += 1;
                        entries.push(Entry {
                            seq,
                            node,
                            op: "@budget".to_string(),
                            port,
                            value: Value::Int(left as i64),
                            kind: Kind::Budget,
                        });
                    }
                }
                parent.cursor = 0;
                parent.fired_in_pass = false;
            } else {
                stack[idx].cursor = 0;
                stack[idx].fired_in_pass = false;
            }
            continue;
        };
        stack[idx].cursor = at + 1;
        match action {
            Action::Fire {
                name,
                op,
                slot,
                out,
                marks,
            } => {
                let f = &mut stack[idx];
                if trace {
                    println!("{}{} ({}) -> {}", "  ".repeat(idx), name, op, out);
                }
                seq += 1;
                entries.push(Entry {
                    seq,
                    node: name.clone(),
                    op: op.clone(),
                    port: slot.1.clone(),
                    value: out.clone(),
                    kind: Kind::Fire,
                });
                f.values.insert(slot.clone(), out.clone());
                *f.stamps.entry(slot.clone()).or_insert(0) += 1;
                f.fired.insert(name.clone(), marks);
                f.order.push(name);
                f.fired_in_pass = true;
                let spent = deliver(
                    f.g,
                    &slot,
                    &out,
                    &mut f.values,
                    &mut f.stamps,
                    &mut f.budgets,
                )?;
                for ((node, port), left) in spent {
                    seq += 1;
                    entries.push(Entry {
                        seq,
                        node,
                        op: "@budget".to_string(),
                        port,
                        value: Value::Int(left as i64),
                        kind: Kind::Budget,
                    });
                }
                fires += 1;
                if fires > 1_000_000 {
                    return Err(
                        "the run passed 1,000,000 firings; a ring is not terminating".to_string(),
                    );
                }
            }
            Action::Call {
                name,
                op,
                marks,
                inputs,
                body,
            } => {
                let mut seed_child: BTreeMap<(String, String), Value> = BTreeMap::new();
                for (node, port) in &body.params {
                    let v = inputs
                        .get(port)
                        .cloned()
                        .ok_or_else(|| format!("`{op}` is missing its `{port}` argument"))?;
                    seed_child.insert((node.clone(), port.clone()), v);
                }
                {
                    let f = &mut stack[idx];
                    f.pending = Some((name.clone(), marks));
                }
                if stack.len() > 1000 {
                    return Err(format!(
                        "`{op}` went more than 1000 frames deep; the recursion is not terminating"
                    ));
                }
                let caller = (name, body.outs.clone());
                let child = Frame::new(&body.graph, &seed_child, &BTreeMap::new(), Some(caller));
                stack.push(child);
            }
        }
    }

    let root = stack.pop().expect("the root frame");
    let skipped: Vec<String> = root
        .g
        .nodes
        .iter()
        .filter(|n| !root.fired.contains_key(&n.name) && !root.seeded.contains(&n.name))
        .map(|n| n.name.clone())
        .collect();
    Ok(Run {
        values: root.values,
        order: root.order,
        skipped,
        entries,
    })
}

/// `match` runs the arm named after the value's tag, seeding the arm's
/// parameters from the payload fields of the same name.
fn run_match(
    subs: &BTreeMap<&str, &SubGraph>,
    inputs: &BTreeMap<&str, Value>,
) -> Result<Value, String> {
    let v = inputs
        .get("on")
        .ok_or_else(|| "`match` has no `on` value".to_string())?;
    let (tag, fields) = match v {
        Value::Variant { tag, fields } => (tag, fields),
        other => {
            return Err(format!(
                "`match` wants a variant, got {}",
                type_of(other).name()
            ));
        }
    };
    let sub = subs
        .get(tag.as_str())
        .ok_or_else(|| format!("`match` has no arm for `{tag}`"))?;
    let mut seed = BTreeMap::new();
    for (param, port) in &sub.inputs {
        let v = fields
            .get(param)
            .cloned()
            .ok_or_else(|| format!("`match.{tag}` wants the field `{param}`"))?;
        seed.insert((param.clone(), port.clone()), v);
    }
    let run = run_from(&sub.graph, false, &seed, &BTreeMap::new(), 0)?;
    run.values
        .get(&sub.output)
        .cloned()
        .ok_or_else(|| format!("`match.{tag}`: the arm produced no `{}`", sub.output.0))
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
                let run = run_from(&sub.graph, false, &seed, &BTreeMap::new(), 0)?;
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
                let run = run_from(&sub.graph, false, &seed, &BTreeMap::new(), 0)?;
                acc =
                    run.values.get(&sub.output).cloned().ok_or_else(|| {
                        format!("`fold`: the body produced no `{}`", sub.output.0)
                    })?;
            }
            Ok(acc)
        }
        "filter" | "any" | "all" => {
            let mut kept = Vec::new();
            let mut any_hit = false;
            let mut all_hit = true;
            for item in xs {
                let mut seed = BTreeMap::new();
                seed.insert(sub.inputs[0].clone(), item.clone());
                let run = run_from(&sub.graph, false, &seed, &BTreeMap::new(), 0)?;
                let v =
                    run.values.get(&sub.output).cloned().ok_or_else(|| {
                        format!("`{op}`: the body produced no `{}`", sub.output.0)
                    })?;
                let hit = match v {
                    Value::Bool(b) => b,
                    other => {
                        return Err(format!(
                            "`{op}` wants a Bool from the body, got {}",
                            type_of(&other).name()
                        ));
                    }
                };
                if hit {
                    any_hit = true;
                    kept.push(item.clone());
                } else {
                    all_hit = false;
                }
            }
            match op {
                "filter" => Ok(Value::List(kept)),
                "any" => Ok(Value::Bool(any_hit)),
                _ => Ok(Value::Bool(all_hit)),
            }
        }
        other => Err(format!("`{other}` does not run a def per element")),
    }
}

/// A back edge just carried a value, and this much budget is left on it.
type Spent = Vec<((String, String), u32)>;

fn deliver(
    g: &Graph,
    src: &(String, String),
    v: &Value,
    values: &mut BTreeMap<(String, String), Value>,
    stamps: &mut BTreeMap<(String, String), u64>,
    budgets: &mut [Option<u32>],
) -> Result<Spent, String> {
    let ids: Vec<usize> = g
        .edges
        .iter()
        .enumerate()
        .filter(|(_, e)| &e.from == src)
        .map(|(i, _)| i)
        .collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let exclusive = ids
        .iter()
        .any(|i| matches!(g.edges[*i].guard, Guard::When(_) | Guard::Fallback));
    let mut chosen: Vec<usize> = Vec::new();
    if exclusive {
        for i in &ids {
            if let Guard::When(expr) = &g.edges[*i].guard
                && truthy(&eval(expr, &src.0, values)?)?
            {
                chosen.push(*i);
                break;
            }
        }
        if chosen.is_empty()
            && let Some(i) = ids
                .iter()
                .find(|i| matches!(g.edges[**i].guard, Guard::Fallback))
        {
            chosen.push(*i);
        }
    } else {
        chosen = ids;
    }

    let mut spent: Spent = Vec::new();
    for i in chosen {
        let edge = &g.edges[i];
        if edge.ring
            && let Some(room) = &mut budgets[i]
        {
            if *room == 0 {
                continue;
            }
            *room -= 1;
            spent.push((edge.to.clone(), *room));
        }
        values.insert(edge.to.clone(), v.clone());
        *stamps.entry(edge.to.clone()).or_insert(0) += 1;
    }
    Ok(spent)
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

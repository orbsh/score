//! Parser: line-grouped tokens into a graph.
//!
//! The chain state is `(head, current)`: a line starting with `->` continues `current`,
//! `-->` and `-[expr]->` restart from `head`.
//!
//! `def` bodies are parsed into their own graph and inlined at the call site, so the
//! graph that leaves this module has no subgraphs in it.

use std::collections::BTreeMap;

use crate::ast::{Arg, Attrs, BinOp, Ctor, Edge, Expr, Graph, Guard, Node};
use crate::lexer::{Tok, lex};
use crate::ops;
use crate::value::{Ty, Value, accepts, type_of};

struct Def {
    params: Vec<(String, Ty)>,
    outs: Vec<(String, Ty)>,
    body: Graph,
}

pub fn parse(src: &str) -> Result<Graph, String> {
    let toks = lex(src)?;
    let mut lines: Vec<Vec<Tok>> = Vec::new();
    let mut cur: Vec<Tok> = Vec::new();
    let mut cur_line = 0usize;
    for (line, tok) in toks {
        if cur_line != 0 && line != cur_line && !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
        }
        cur_line = line;
        cur.push(tok);
    }
    if !cur.is_empty() {
        lines.push(cur);
    }

    let mut decls = Decls::default();
    let mut consts: BTreeMap<String, Value> = BTreeMap::new();
    let mut g = parse_lines(&lines, &mut decls, &mut consts, None)?;
    g.ctors = decls.ctors;
    g.structs = decls.structs;
    g.decl_attrs = decls.attrs;
    g.sigs = decls
        .defs
        .iter()
        .map(|(name, d)| (name.clone(), (d.params.clone(), d.outs.clone())))
        .collect();
    Ok(g)
}

/// Names a program declares before its graph: subgraphs, sum constructors and
/// structs. All three are visible inside a `def` body, so they travel together.
#[derive(Default)]
struct Decls {
    defs: BTreeMap<String, Def>,
    ctors: BTreeMap<String, Ctor>,
    structs: BTreeMap<String, Vec<(String, Ty)>>,
    attrs: BTreeMap<String, Attrs>,
}

/// `@doc("…")` / `@tag(x)` written in front of an element. An attribute is a
/// compile-time tag: it never becomes a value and never changes what a run
/// does, so it stays out of the fingerprint.
fn parse_attrs(toks: &[Tok]) -> Result<(Attrs, usize), String> {
    let mut out = BTreeMap::new();
    let mut i = 0usize;
    while matches!(toks.get(i), Some(Tok::At)) {
        i += 1;
        let name = match toks.get(i) {
            Some(Tok::Ident(n)) => n.clone(),
            other => return Err(format!("`@` needs a name, got {other:?}")),
        };
        i += 1;
        let mut value = None;
        if matches!(toks.get(i), Some(Tok::LParen)) {
            i += 1;
            match toks.get(i) {
                Some(Tok::Str(s)) => {
                    value = Some(s.clone());
                    i += 1;
                }
                Some(Tok::Ident(s)) if s != "true" && s != "false" => {
                    value = Some(s.clone());
                    i += 1;
                }
                Some(Tok::RParen) => {}
                other => {
                    return Err(format!("`@{name}` takes one value, got {other:?}"));
                }
            }
            if !matches!(toks.get(i), Some(Tok::RParen)) {
                return Err(format!("`@{name}` takes one value"));
            }
            i += 1;
        }
        if !matches!(name.as_str(), "doc" | "tag") {
            return Err(format!(
                "unknown attribute `@{name}`; known attributes are `@doc` and `@tag`"
            ));
        }
        out.insert(name, value);
    }
    Ok((out, i))
}

fn parse_lines(
    lines: &[Vec<Tok>],
    defs: &mut Decls,
    consts: &mut BTreeMap<String, Value>,
    params: Option<&[(String, Ty)]>,
) -> Result<Graph, String> {
    let mut g = Graph::default();
    if let Some(ps) = params {
        for (name, _) in ps {
            g.nodes.push(Node {
                name: name.clone(),
                op: "param".to_string(),
                args: BTreeMap::new(),
                out_port: name.clone(),
            });
        }
    }

    let mut head: Option<(String, String)> = None;
    let mut current: Option<(String, String)> = None;
    let mut auto = 0usize;

    let mut pending: Attrs = BTreeMap::new();
    let mut i = 0usize;
    while i < lines.len() {
        let (line_attrs, off) = parse_attrs(&lines[i])?;
        let line = &lines[i][off..];
        if line.is_empty() {
            // Attributes on a line of their own decorate whatever comes next.
            pending.extend(line_attrs);
            i += 1;
            continue;
        }
        let mut attrs = std::mem::take(&mut pending);
        attrs.extend(line_attrs);
        let first_node = g.nodes.len();
        match line[0].clone() {
            Tok::Const => {
                let name = match line.get(1) {
                    Some(Tok::Ident(n)) => n.clone(),
                    _ => return Err("`const` needs a name".to_string()),
                };
                if !matches!(line.get(2), Some(Tok::Eq)) {
                    return Err(format!("`const {name}` needs `=`"));
                }
                let value = literal(line.get(3))
                    .ok_or_else(|| format!("`const {name}` needs a literal"))?;
                consts.insert(name, value);
            }
            Tok::Def => {
                if params.is_some() {
                    return Err("a `def` cannot be nested".to_string());
                }
                let (name, def_params, def_outs) = parse_def_header(&line[1..])?;
                let mut j = i + 1;
                while j < lines.len() && !(lines[j].len() == 1 && lines[j][0] == Tok::RBrace) {
                    j += 1;
                }
                if j == lines.len() {
                    return Err(format!("`def {name}` has no closing `}}`"));
                }
                let body = parse_lines(&lines[i + 1..j], defs, consts, Some(&def_params))?;
                let decorated = std::mem::take(&mut attrs);
                if !decorated.is_empty() {
                    defs.attrs.insert(name.clone(), decorated);
                }
                defs.defs.insert(
                    name,
                    Def {
                        params: def_params,
                        outs: def_outs,
                        body,
                    },
                );
                i = j;
            }
            Tok::LBrace | Tok::RBrace => {
                return Err("unexpected brace at the top level".to_string());
            }
            Tok::Arrow | Tok::Branch | Tok::CondOpen => {
                apply_arrow(&mut g, line, &head, &mut current, consts, defs, &mut auto)?;
            }
            Tok::Ident(word) if word == "enum" => {
                let name = match line.get(1) {
                    Some(Tok::Ident(n)) => n.clone(),
                    _ => return Err("`enum` needs a name".to_string()),
                };
                if !matches!(line.get(2), Some(Tok::LBrace)) {
                    return Err(format!("`enum {name}` needs `{{`"));
                }
                let mut j = i + 1;
                let mut tags: Vec<String> = Vec::new();
                while j < lines.len() && !(lines[j].len() == 1 && lines[j][0] == Tok::RBrace) {
                    let v = &lines[j];
                    let tag = match v.first() {
                        Some(Tok::Ident(t)) => t.clone(),
                        other => {
                            return Err(format!(
                                "`enum {name}`: expected a variant, got {other:?}"
                            ));
                        }
                    };
                    let fields = match v.get(1) {
                        Some(Tok::LBrace) => {
                            if v.last() != Some(&Tok::RBrace) {
                                return Err(format!(
                                    "`enum {name}.{tag}`: close its fields on the same line"
                                ));
                            }
                            parse_fields(&v[2..v.len() - 1])?
                        }
                        Some(other) => {
                            return Err(format!("`enum {name}.{tag}`: unexpected {other:?}"));
                        }
                        None => Vec::new(),
                    };
                    tags.push(tag.clone());
                    defs.ctors.insert(
                        tag,
                        Ctor {
                            sum: name.clone(),
                            fields,
                        },
                    );
                    j += 1;
                }
                if j == lines.len() {
                    return Err(format!("`enum {name}` has no closing `}}`"));
                }
                if tags.is_empty() {
                    return Err(format!("`enum {name}` has no variants"));
                }
                let decorated = std::mem::take(&mut attrs);
                if !decorated.is_empty() {
                    defs.attrs.insert(name, decorated);
                }
                i = j;
            }
            Tok::Ident(word) if word == "struct" => {
                let name = match line.get(1) {
                    Some(Tok::Ident(n)) => n.clone(),
                    _ => return Err("`struct` needs a name".to_string()),
                };
                if !matches!(line.get(2), Some(Tok::LBrace)) {
                    return Err(format!("`struct {name}` needs `{{`"));
                }
                let mut j = i + 1;
                let mut fields: Vec<(String, Ty)> = Vec::new();
                while j < lines.len() && !(lines[j].len() == 1 && lines[j][0] == Tok::RBrace) {
                    fields.extend(parse_fields(&lines[j])?);
                    j += 1;
                }
                if j == lines.len() {
                    return Err(format!("`struct {name}` has no closing `}}`"));
                }
                defs.structs.insert(name.clone(), fields);
                let decorated = std::mem::take(&mut attrs);
                if !decorated.is_empty() {
                    defs.attrs.insert(name, decorated);
                }
                i = j;
            }
            Tok::Ident(name) => match line.get(1) {
                Some(Tok::Eq) => {
                    let (step, used) = parse_step(
                        &mut g,
                        &line[2..],
                        consts,
                        defs,
                        &mut auto,
                        Some(name.clone()),
                        None,
                    )?;
                    if used != line[2..].len() {
                        return Err(format!("trailing tokens after the call to `{name}`"));
                    }
                    head = Some((step.name.clone(), step.out_port.clone()));
                    current = head.clone();
                }
                Some(Tok::LParen) | Some(Tok::Lt) => {
                    let (step, used) =
                        parse_step(&mut g, line, consts, defs, &mut auto, None, None)?;
                    if used != line.len() {
                        return Err(format!("trailing tokens after the call to `{}`", step.name));
                    }
                    head = Some((step.name.clone(), step.out_port.clone()));
                    current = head.clone();
                }
                Some(Tok::Arrow) | Some(Tok::Branch) | Some(Tok::CondOpen) => {
                    let node = g
                        .node(&name)
                        .ok_or_else(|| format!("unknown node `{name}`"))?;
                    let pair = (name.clone(), node.out_port.clone());
                    head = Some(pair.clone());
                    current = Some(pair);
                    apply_arrow(
                        &mut g,
                        &line[1..],
                        &head,
                        &mut current,
                        consts,
                        defs,
                        &mut auto,
                    )?;
                }
                None => {
                    let node = g
                        .node(&name)
                        .ok_or_else(|| format!("unknown node `{name}`"))?;
                    let pair = (name.clone(), node.out_port.clone());
                    head = Some(pair.clone());
                    current = Some(pair);
                }
                other => return Err(format!("unexpected token after `{name}`: {other:?}")),
            },
            other => return Err(format!("unexpected start of line: {other:?}")),
        }
        if !attrs.is_empty() {
            if g.nodes.len() == first_node {
                return Err("an attribute decorated nothing on this line".to_string());
            }
            let target = g.nodes[first_node].name.clone();
            g.node_attrs.insert(target, attrs);
        }
        i += 1;
    }
    if !pending.is_empty() {
        return Err("an attribute decorated nothing".to_string());
    }
    Ok(g)
}

fn apply_arrow(
    g: &mut Graph,
    toks: &[Tok],
    head: &Option<(String, String)>,
    current: &mut Option<(String, String)>,
    consts: &BTreeMap<String, Value>,
    defs: &Decls,
    auto: &mut usize,
) -> Result<(), String> {
    match toks.first() {
        Some(Tok::Arrow) => {
            let start = current.clone().ok_or("`->` has no path to continue")?;
            *current = Some(run_steps(
                g,
                &toks[1..],
                start,
                consts,
                defs,
                auto,
                FirstEdge {
                    guard: Guard::Always,
                    ring: false,
                    budget: None,
                },
            )?);
        }
        Some(Tok::Branch) => {
            let start = head.clone().ok_or("`-->` has no head")?;
            *current = Some(run_steps(
                g,
                &toks[1..],
                start,
                consts,
                defs,
                auto,
                FirstEdge {
                    guard: Guard::Always,
                    ring: false,
                    budget: None,
                },
            )?);
        }
        Some(Tok::CondOpen) => {
            let close = toks
                .iter()
                .position(|t| *t == Tok::CondClose)
                .ok_or("a bracket is not closed with `]->`")?;
            let start = head.clone().ok_or("this edge has no head")?;
            match parse_bracket(&toks[1..close], consts)? {
                Bracket::Cond(guard) => {
                    *current = Some(run_steps(
                        g,
                        &toks[close + 1..],
                        start,
                        consts,
                        defs,
                        auto,
                        FirstEdge {
                            guard,
                            ring: false,
                            budget: None,
                        },
                    )?);
                }
                Bracket::Budget(budget) => {
                    *current = Some(run_steps(
                        g,
                        &toks[close + 1..],
                        start,
                        consts,
                        defs,
                        auto,
                        FirstEdge {
                            guard: Guard::Always,
                            ring: true,
                            budget,
                        },
                    )?);
                }
            }
        }
        other => return Err(format!("expected an arrow, got {other:?}")),
    }
    Ok(())
}

struct Step {
    name: String,
    out_port: String,
    in_port: Option<String>,
    inlined: bool,
}

enum Bracket {
    Cond(Guard),
    Budget(Option<u32>),
}

/// The brackets after `-` hold either a condition or a loop budget:
/// `-[out > 5]->` and `-[*]->` are arms, `-[budget: 1000]->` is a back edge.
fn parse_bracket(toks: &[Tok], consts: &BTreeMap<String, Value>) -> Result<Bracket, String> {
    if let [Tok::Ident(key), Tok::Colon, rest @ ..] = toks
        && key == "budget"
    {
        return match rest {
            [Tok::Int(n)] if *n >= 0 => Ok(Bracket::Budget(Some(*n as u32))),
            [Tok::Ident(word)] if word == "unbounded" => Ok(Bracket::Budget(None)),
            _ => Err("`budget` wants a count or `unbounded`".to_string()),
        };
    }
    if toks == [Tok::Star] {
        return Ok(Bracket::Cond(Guard::Fallback));
    }
    Ok(Bracket::Cond(Guard::When(parse_expr(toks, consts)?)))
}

struct FirstEdge {
    guard: Guard,
    ring: bool,
    budget: Option<u32>,
}

fn run_steps(
    g: &mut Graph,
    toks: &[Tok],
    start: (String, String),
    consts: &BTreeMap<String, Value>,
    defs: &Decls,
    auto: &mut usize,
    first: FirstEdge,
) -> Result<(String, String), String> {
    if toks.is_empty() {
        return Err("an arrow needs a step after it".to_string());
    }
    let mut src = start;
    let mut rest = toks;
    let mut first_step = true;
    loop {
        let (step, used) = parse_step(g, rest, consts, defs, auto, None, Some(&src))?;
        if !step.inlined {
            let in_port = step.in_port.clone().ok_or_else(|| {
                format!(
                    "`{}` needs an explicit entry port: write `<in|out>`",
                    step.name
                )
            })?;
            let guard = if first_step {
                first.guard.clone()
            } else {
                Guard::Always
            };
            g.edges.push(Edge {
                from: src.clone(),
                to: (step.name.clone(), in_port),
                guard,
                ring: first_step && first.ring,
                budget: if first_step { first.budget } else { None },
            });
        }
        src = (step.name.clone(), step.out_port.clone());
        rest = &rest[used..];
        first_step = false;
        if rest.is_empty() {
            break;
        }
        match rest[0] {
            Tok::Arrow => rest = &rest[1..],
            ref other => return Err(format!("unexpected token after a step: {other:?}")),
        }
    }
    Ok(src)
}

fn parse_step(
    g: &mut Graph,
    toks: &[Tok],
    consts: &BTreeMap<String, Value>,
    defs: &Decls,
    auto: &mut usize,
    hint: Option<String>,
    entry: Option<&(String, String)>,
) -> Result<(Step, usize), String> {
    let mut i = 0usize;
    let mut binding = hint;
    if binding.is_none()
        && let (Some(Tok::Ident(n)), Some(Tok::Eq)) = (toks.first(), toks.get(1))
    {
        binding = Some(n.clone());
        i = 2;
    }
    let op = match toks.get(i) {
        Some(Tok::Ident(n)) => n.clone(),
        other => return Err(format!("expected an operator, got {other:?}")),
    };
    i += 1;
    let name = match binding.take() {
        Some(n) => n,
        None => {
            *auto += 1;
            format!("{op}#{}", *auto)
        }
    };

    let mut spec_in = None;
    let mut spec_out = None;
    if matches!(toks.get(i), Some(Tok::Lt)) {
        i += 1;
        if matches!(toks.get(i), Some(Tok::Pipe)) {
            i += 1;
        } else {
            match toks.get(i) {
                Some(Tok::Ident(n)) => {
                    spec_in = Some(n.clone());
                    i += 1;
                }
                other => return Err(format!("expected an input port, got {other:?}")),
            }
            match toks.get(i) {
                Some(Tok::Pipe) => i += 1,
                other => return Err(format!("expected `|` in the port spec, got {other:?}")),
            }
        }
        if !matches!(toks.get(i), Some(Tok::Gt)) {
            match toks.get(i) {
                Some(Tok::Ident(n)) => {
                    spec_out = Some(n.clone());
                    i += 1;
                }
                other => return Err(format!("expected an output port, got {other:?}")),
            }
        }
        match toks.get(i) {
            Some(Tok::Gt) => i += 1,
            other => {
                return Err(format!(
                    "expected `>` to close the port spec, got {other:?}"
                ));
            }
        }
    }

    let mut args: BTreeMap<String, Arg> = BTreeMap::new();
    if matches!(toks.get(i), Some(Tok::LParen)) {
        i += 1;
        loop {
            if matches!(toks.get(i), Some(Tok::RParen)) {
                i += 1;
                break;
            }
            let port = match toks.get(i) {
                Some(Tok::Ident(n)) => n.clone(),
                other => return Err(format!("expected an argument name, got {other:?}")),
            };
            if !matches!(toks.get(i + 1), Some(Tok::Colon)) {
                return Err(format!("expected `:` after `{port}`"));
            }
            i += 2;
            let tok = toks
                .get(i)
                .ok_or_else(|| format!("missing value for `{port}`"))?;
            let arg = match tok {
                Tok::Int(n) => Arg::Lit(Value::Int(*n)),
                Tok::Str(s) => Arg::Lit(Value::Text(s.clone())),
                Tok::Ident(n) if n == "true" => Arg::Lit(Value::Bool(true)),
                Tok::Ident(n) if n == "false" => Arg::Lit(Value::Bool(false)),
                Tok::Ident(n) => match consts.get(n) {
                    Some(v) => Arg::Lit(v.clone()),
                    None => Arg::Ref(n.clone()),
                },
                other => return Err(format!("bad argument value {other:?}")),
            };
            args.insert(port, arg);
            i += 1;
            match toks.get(i) {
                Some(Tok::Comma) => i += 1,
                Some(Tok::RParen) => {
                    i += 1;
                    break;
                }
                other => return Err(format!("expected `,` or `)`, got {other:?}")),
            }
        }
    }

    if let Some(def) = defs.defs.get(&op) {
        // The entry port comes from the port spec, or from the single parameter.
        let entry_port = match &spec_in {
            Some(p) => Some(p.clone()),
            None if def.params.len() == 1 => Some(def.params[0].0.clone()),
            None => None,
        };
        let chained = match (&entry_port, entry) {
            (Some(p), Some(slot)) => {
                if args.contains_key(p) {
                    return Err(format!(
                        "`{op}.{p}` has both an argument and a chained value"
                    ));
                }
                if !def.params.iter().any(|(name, _)| name == p) {
                    return Err(format!("`{op}` has no input `{p}`"));
                }
                Some((p.clone(), slot.clone()))
            }
            _ => None,
        };
        let out_port = inline_def(
            g,
            def,
            &op,
            &name,
            &args,
            spec_out.as_deref(),
            chained.as_ref(),
        )?;
        return Ok((
            Step {
                name,
                out_port,
                in_port: None,
                inlined: true,
            },
            i,
        ));
    }

    if ops::signature(&op).is_none()
        && let Some(node) = g.node(&op)
    {
        let out_port = node.out_port.clone();
        let node_op = node.op.clone();
        let sig_of =
            ops::signature(&node_op).ok_or_else(|| format!("`{node_op}` is not an operator"))?;
        let in_port = spec_in.or_else(|| ops::default_in(sig_of).map(str::to_string));
        return Ok((
            Step {
                name: op,
                out_port,
                in_port,
                inlined: false,
            },
            i,
        ));
    }

    if let Some(ctor) = defs.ctors.get(&op) {
        check_fields(g, &op, &ctor.fields, &args)?;
        g.nodes.push(Node {
            name: name.clone(),
            op: op.clone(),
            args,
            out_port: op.clone(),
        });
        return Ok((
            Step {
                name,
                out_port: op,
                in_port: None,
                inlined: false,
            },
            i,
        ));
    }

    if let Some(fields) = defs.structs.get(&op) {
        check_fields(g, &op, fields, &args)?;
        g.nodes.push(Node {
            name: name.clone(),
            op: op.clone(),
            args,
            out_port: "out".to_string(),
        });
        return Ok((
            Step {
                name,
                out_port: "out".to_string(),
                in_port: None,
                inlined: false,
            },
            i,
        ));
    }

    let container_arity = match op.as_str() {
        "map" | "filter" | "any" | "all" => Some(1),
        "fold" => Some(2),
        _ => None,
    };
    if let Some(want) = container_arity {
        let sub = match args.get("f") {
            Some(Arg::Ref(target)) => match defs.defs.get(target) {
                Some(def) => {
                    if def.params.len() != want {
                        return Err(format!(
                            "`{op}` wants a def of {want} parameter(s), `{target}` has {}",
                            def.params.len()
                        ));
                    }
                    subgraph_from(def, &name, &op)?
                }
                None => return Err(format!("`{op}` needs a def as `f`; `{target}` is not one")),
            },
            _ => return Err(format!("`{op}` needs `f: <def>`")),
        };
        args.insert("f".to_string(), Arg::Sub(Box::new(sub)));
    }

    let sig = ops::signature(&op).ok_or_else(|| format!("unknown operator or def `{op}`"))?;
    let in_port = spec_in.or_else(|| ops::default_in(sig).map(str::to_string));
    let out_port = spec_out
        .or_else(|| ops::default_out(sig).map(str::to_string))
        .ok_or_else(|| format!("`{op}` has no default output port: write `<|port>`"))?;
    g.nodes.push(Node {
        name: name.clone(),
        op,
        args,
        out_port: out_port.clone(),
    });
    Ok((
        Step {
            name,
            out_port,
            in_port,
            inlined: false,
        },
        i,
    ))
}

/// Inline a subgraph at the call site and return the out port of the node that
/// carries the result (the node is renamed to the call's binding).
fn inline_def(
    g: &mut Graph,
    def: &Def,
    op_name: &str,
    bind: &str,
    args: &BTreeMap<String, Arg>,
    spec_out: Option<&str>,
    chained: Option<&(String, (String, String))>,
) -> Result<String, String> {
    let out_name = match spec_out {
        Some(name) => {
            if !def.outs.iter().any(|(o, _)| o == name) {
                return Err(format!("`{op_name}` has no output `{name}`"));
            }
            name.to_string()
        }
        None if def.outs.len() == 1 => def.outs[0].0.clone(),
        None => {
            let names: Vec<&str> = def.outs.iter().map(|(o, _)| o.as_str()).collect();
            return Err(format!(
                "`{op_name}` has several outputs ({}); pick one with `<|name>`",
                names.join(", ")
            ));
        }
    };

    for (p, ty) in &def.params {
        if let Some((name, slot)) = chained
            && name == p
        {
            let sn = g
                .node(&slot.0)
                .ok_or_else(|| format!("unknown node `{}`", slot.0))?;
            let ssig = ops::signature(&sn.op)
                .ok_or_else(|| format!("`{}` cannot feed `{op_name}.{p}`", sn.op))?;
            let got = ssig
                .outs
                .iter()
                .find(|(o, _)| *o == slot.1)
                .map(|(_, t)| *t)
                .ok_or_else(|| format!("`{}` has no output `{}`", sn.op, slot.1))?;
            if !accepts(*ty, got) {
                return Err(format!(
                    "`{op_name}.{p}` wants {}, got {}",
                    ty.name(),
                    got.name()
                ));
            }
            continue;
        }
        let arg = args
            .get(p)
            .ok_or_else(|| format!("`{op_name}` needs `{p}: {}`", ty.name()))?;
        let got = match arg {
            Arg::Lit(v) => type_of(v),
            Arg::Sub(_) => Ty::Any,
            Arg::Ref(r) => {
                let rn = g.node(r).ok_or_else(|| format!("unknown node `{r}`"))?;
                let rsig = ops::signature(&rn.op)
                    .ok_or_else(|| format!("`{}` cannot feed `{op_name}.{p}`", rn.op))?;
                rsig.outs
                    .iter()
                    .find(|(o, _)| *o == rn.out_port)
                    .map(|(_, t)| *t)
                    .ok_or_else(|| format!("`{}` has no output `{}`", rn.op, rn.out_port))?
            }
        };
        if !accepts(*ty, got) {
            return Err(format!(
                "`{op_name}.{p}` wants {}, got {}",
                ty.name(),
                got.name()
            ));
        }
    }
    for p in args.keys() {
        if !def.params.iter().any(|(name, _)| name == p) {
            return Err(format!("`{op_name}` has no input `{p}`"));
        }
    }

    let mut rename: BTreeMap<String, String> = BTreeMap::new();
    for n in &def.body.nodes {
        let new_name = if n.name == out_name {
            bind.to_string()
        } else {
            format!("{bind}/{}", n.name)
        };
        rename.insert(n.name.clone(), new_name);
    }

    // Param sources: a literal becomes a lit node, a reference or the chained
    // slot is wired directly.
    let mut resolved: BTreeMap<String, (String, String)> = BTreeMap::new();
    for (p, _) in &def.params {
        if let Some((name, slot)) = chained
            && name == p
        {
            resolved.insert(p.clone(), slot.clone());
            continue;
        }
        match args.get(p) {
            Some(Arg::Ref(r)) => {
                let rn = g.node(r).ok_or_else(|| format!("unknown node `{r}`"))?;
                resolved.insert(p.clone(), (r.clone(), rn.out_port.clone()));
            }
            _ => {
                resolved.insert(p.clone(), (rename[p].clone(), "out".to_string()));
            }
        }
    }

    for n in &def.body.nodes {
        if n.op == "param" {
            if let Some((name, _)) = chained
                && *name == n.name
            {
                continue;
            }
            match args.get(&n.name) {
                Some(Arg::Ref(_)) => continue,
                _ => {
                    let mut args2 = BTreeMap::new();
                    args2.insert(
                        "value".to_string(),
                        args.get(&n.name).cloned().unwrap_or(Arg::Lit(Value::Unit)),
                    );
                    g.nodes.push(Node {
                        name: rename[&n.name].clone(),
                        op: "lit".to_string(),
                        args: args2,
                        out_port: "out".to_string(),
                    });
                }
            }
            continue;
        }
        let mut args2 = BTreeMap::new();
        for (port, arg) in &n.args {
            let arg2 = match arg {
                Arg::Lit(v) => Arg::Lit(v.clone()),
                Arg::Sub(sub) => Arg::Sub(sub.clone()),
                Arg::Ref(r) => match resolved.get(r) {
                    Some((node, _)) => Arg::Ref(node.clone()),
                    None => Arg::Ref(
                        rename
                            .get(r)
                            .cloned()
                            .ok_or_else(|| format!("`{r}` is not a node of `{op_name}`"))?,
                    ),
                },
            };
            args2.insert(port.clone(), arg2);
        }
        g.nodes.push(Node {
            name: rename[&n.name].clone(),
            op: n.op.clone(),
            args: args2,
            out_port: n.out_port.clone(),
        });
    }

    for e in &def.body.edges {
        let from = match resolved.get(&e.from.0) {
            Some(pair) => pair.clone(),
            None => (
                rename
                    .get(&e.from.0)
                    .cloned()
                    .ok_or_else(|| format!("`{}` is not a node of `{op_name}`", e.from.0))?,
                e.from.1.clone(),
            ),
        };
        let to = (
            rename
                .get(&e.to.0)
                .cloned()
                .ok_or_else(|| format!("`{}` is not a node of `{op_name}`", e.to.0))?,
            e.to.1.clone(),
        );
        g.edges.push(Edge {
            from,
            to,
            guard: e.guard.clone(),
            ring: e.ring,
            budget: e.budget,
        });
    }

    let out_node = def
        .body
        .node(&out_name)
        .ok_or_else(|| format!("`{op_name}` never binds its output `{out_name}`"))?;
    Ok(out_node.out_port.clone())
}

fn parse_fields(toks: &[Tok]) -> Result<Vec<(String, Ty)>, String> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < toks.len() {
        let name = match toks.get(i) {
            Some(Tok::Ident(n)) => n.clone(),
            other => return Err(format!("expected a field name, got {other:?}")),
        };
        if !matches!(toks.get(i + 1), Some(Tok::Colon)) {
            return Err(format!("field `{name}` needs `: Type`"));
        }
        let ty = parse_ty(toks.get(i + 2))?;
        out.push((name, ty));
        i += 3;
        match toks.get(i) {
            Some(Tok::Comma) => i += 1,
            None => break,
            Some(other) => return Err(format!("expected `,` after a field, got {other:?}")),
        }
    }
    Ok(out)
}

type DefSig = (String, Vec<(String, Ty)>, Vec<(String, Ty)>);

/// Clone a def body as a template for `map` / `fold`: names are prefixed, the
/// param nodes stay (the runtime seeds them), and nothing is wired to a caller.
fn check_fields(
    g: &Graph,
    op: &str,
    fields: &[(String, Ty)],
    args: &BTreeMap<String, Arg>,
) -> Result<(), String> {
    for (f, ty) in fields {
        let arg = args
            .get(f)
            .ok_or_else(|| format!("`{op}` needs `{f}: {}`", ty.name()))?;
        let got = match arg {
            Arg::Lit(v) => type_of(v),
            Arg::Sub(_) => Ty::Any,
            Arg::Ref(r) => {
                let rn = g.node(r).ok_or_else(|| format!("unknown node `{r}`"))?;
                match ops::signature(&rn.op) {
                    Some(sig) => sig
                        .outs
                        .iter()
                        .find(|(p, _)| *p == rn.out_port)
                        .map(|(_, t)| *t)
                        .ok_or_else(|| format!("`{}` has no output `{}`", rn.op, rn.out_port))?,
                    None => Ty::Any,
                }
            }
        };
        if !accepts(*ty, got) {
            return Err(format!(
                "`{op}.{f}` wants {}, got {}",
                ty.name(),
                got.name()
            ));
        }
    }
    for a in args.keys() {
        if !fields.iter().any(|(f, _)| f == a) {
            return Err(format!("`{op}` has no field `{a}`"));
        }
    }
    Ok(())
}

fn subgraph_from(def: &Def, prefix: &str, op: &str) -> Result<crate::ast::SubGraph, String> {
    if def.outs.len() != 1 {
        return Err(format!("`{op}` wants a def with exactly one output"));
    }
    let out_name = def.outs[0].0.clone();
    let mut rename: BTreeMap<String, String> = BTreeMap::new();
    for n in &def.body.nodes {
        rename.insert(n.name.clone(), format!("{prefix}/{}", n.name));
    }

    let mut g = Graph::default();
    for n in &def.body.nodes {
        let mut args = BTreeMap::new();
        for (port, arg) in &n.args {
            let arg = match arg {
                Arg::Lit(v) => Arg::Lit(v.clone()),
                Arg::Ref(r) => Arg::Ref(
                    rename
                        .get(r)
                        .cloned()
                        .ok_or_else(|| format!("`{r}` is not a node of `{prefix}`"))?,
                ),
                Arg::Sub(sub) => Arg::Sub(sub.clone()),
            };
            args.insert(port.clone(), arg);
        }
        g.nodes.push(Node {
            name: rename[&n.name].clone(),
            op: n.op.clone(),
            args,
            out_port: n.out_port.clone(),
        });
    }
    for e in &def.body.edges {
        g.edges.push(Edge {
            from: (rename[&e.from.0].clone(), e.from.1.clone()),
            to: (rename[&e.to.0].clone(), e.to.1.clone()),
            guard: e.guard.clone(),
            ring: e.ring,
            budget: e.budget,
        });
    }

    let out_port = g
        .node(&rename[&out_name])
        .ok_or_else(|| format!("`{op}`: the body never binds its output `{out_name}`"))?
        .out_port
        .clone();
    Ok(crate::ast::SubGraph {
        graph: g,
        inputs: def
            .params
            .iter()
            .map(|(p, _)| (rename[p].clone(), p.clone()))
            .collect(),
        output: (rename[&out_name].clone(), out_port),
    })
}

fn parse_def_header(toks: &[Tok]) -> Result<DefSig, String> {
    let mut i = 0usize;
    let name = match toks.get(i) {
        Some(Tok::Ident(n)) => n.clone(),
        other => return Err(format!("expected a def name, got {other:?}")),
    };
    i += 1;
    if !matches!(toks.get(i), Some(Tok::LParen)) {
        return Err(format!("`def {name}` needs a parameter list"));
    }
    i += 1;
    let mut params: Vec<(String, Ty)> = Vec::new();
    loop {
        match toks.get(i) {
            Some(Tok::RParen) => {
                i += 1;
                break;
            }
            Some(Tok::Ident(p)) => {
                let p = p.clone();
                if !matches!(toks.get(i + 1), Some(Tok::Colon)) {
                    return Err(format!("`def {name}`: `{p}` needs `: Type`"));
                }
                let ty = parse_ty(toks.get(i + 2))?;
                params.push((p, ty));
                i += 3;
                match toks.get(i) {
                    Some(Tok::Comma) => i += 1,
                    Some(Tok::RParen) => {
                        i += 1;
                        break;
                    }
                    other => {
                        return Err(format!("`def {name}`: expected `,` or `)`, got {other:?}"));
                    }
                }
            }
            other => return Err(format!("`def {name}`: expected a parameter, got {other:?}")),
        }
    }
    if !matches!(toks.get(i), Some(Tok::Arrow)) {
        return Err(format!("`def {name}` needs `->` before its outputs"));
    }
    i += 1;
    if !matches!(toks.get(i), Some(Tok::LParen)) {
        return Err(format!("`def {name}` needs an output list"));
    }
    i += 1;
    let mut outs: Vec<(String, Ty)> = Vec::new();
    loop {
        match toks.get(i) {
            Some(Tok::RParen) => break,
            Some(Tok::Ident(o)) => {
                let o = o.clone();
                let ty = if matches!(toks.get(i + 1), Some(Tok::Colon)) {
                    let t = parse_ty(toks.get(i + 2))?;
                    i += 3;
                    t
                } else {
                    i += 1;
                    Ty::Any
                };
                outs.push((o, ty));
                match toks.get(i) {
                    Some(Tok::Comma) => i += 1,
                    Some(Tok::RParen) => break,
                    other => {
                        return Err(format!("`def {name}`: expected `,` or `)`, got {other:?}"));
                    }
                }
            }
            other => {
                return Err(format!(
                    "`def {name}`: expected an output name, got {other:?}"
                ));
            }
        }
    }
    Ok((name, params, outs))
}

fn parse_ty(tok: Option<&Tok>) -> Result<Ty, String> {
    match tok {
        Some(Tok::Ident(n)) => match n.as_str() {
            "Int" => Ok(Ty::Int),
            "Text" => Ok(Ty::Text),
            "Bool" => Ok(Ty::Bool),
            "Unit" => Ok(Ty::Unit),
            "Any" => Ok(Ty::Any),
            other => Err(format!("unknown type `{other}`")),
        },
        other => Err(format!("expected a type, got {other:?}")),
    }
}

fn literal(tok: Option<&Tok>) -> Option<Value> {
    match tok {
        Some(Tok::Int(n)) => Some(Value::Int(*n)),
        Some(Tok::Str(s)) => Some(Value::Text(s.clone())),
        Some(Tok::Ident(n)) if n == "true" => Some(Value::Bool(true)),
        Some(Tok::Ident(n)) if n == "false" => Some(Value::Bool(false)),
        _ => None,
    }
}

fn parse_expr(toks: &[Tok], consts: &BTreeMap<String, Value>) -> Result<Expr, String> {
    let mut p = ExprParser {
        t: toks,
        i: 0,
        consts,
    };
    let e = p.or()?;
    if p.i != toks.len() {
        return Err(format!("unexpected token in a condition: {:?}", toks[p.i]));
    }
    Ok(e)
}

struct ExprParser<'a> {
    t: &'a [Tok],
    i: usize,
    consts: &'a BTreeMap<String, Value>,
}

impl ExprParser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }

    fn eat(&mut self, tok: &Tok) -> bool {
        if self.peek() == Some(tok) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn or(&mut self) -> Result<Expr, String> {
        let mut left = self.and()?;
        while self.eat(&Tok::Or) {
            let right = self.and()?;
            left = Expr::Bin(BinOp::Or, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn and(&mut self) -> Result<Expr, String> {
        let mut left = self.cmp()?;
        while self.eat(&Tok::And) {
            let right = self.cmp()?;
            left = Expr::Bin(BinOp::And, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn cmp(&mut self) -> Result<Expr, String> {
        let mut left = self.add()?;
        loop {
            let op = match self.peek() {
                Some(Tok::EqEq) => BinOp::Eq,
                Some(Tok::Ne) => BinOp::Ne,
                Some(Tok::Lt) => BinOp::Lt,
                Some(Tok::Le) => BinOp::Le,
                Some(Tok::Gt) => BinOp::Gt,
                Some(Tok::Ge) => BinOp::Ge,
                _ => break,
            };
            self.i += 1;
            let right = self.add()?;
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn add(&mut self) -> Result<Expr, String> {
        let mut left = self.mul()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Plus) => BinOp::Add,
                Some(Tok::Minus) => BinOp::Sub,
                _ => break,
            };
            self.i += 1;
            let right = self.mul()?;
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn mul(&mut self) -> Result<Expr, String> {
        let mut left = self.unary()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Star) => BinOp::Mul,
                Some(Tok::Slash) => BinOp::Div,
                _ => break,
            };
            self.i += 1;
            let right = self.unary()?;
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if self.eat(&Tok::Bang) {
            return Ok(Expr::Not(Box::new(self.unary()?)));
        }
        if self.eat(&Tok::Minus) {
            let e = self.unary()?;
            return Ok(Expr::Bin(
                BinOp::Sub,
                Box::new(Expr::Lit(Value::Int(0))),
                Box::new(e),
            ));
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Expr, String> {
        match self.peek().cloned() {
            Some(Tok::Int(n)) => {
                self.i += 1;
                Ok(Expr::Lit(Value::Int(n)))
            }
            Some(Tok::Str(s)) => {
                self.i += 1;
                Ok(Expr::Lit(Value::Text(s)))
            }
            Some(Tok::Ident(n)) => {
                self.i += 1;
                match n.as_str() {
                    "true" => Ok(Expr::Lit(Value::Bool(true))),
                    "false" => Ok(Expr::Lit(Value::Bool(false))),
                    _ => match self.consts.get(&n) {
                        Some(v) => Ok(Expr::Lit(v.clone())),
                        None => Ok(Expr::Local(n)),
                    },
                }
            }
            Some(Tok::LParen) => {
                self.i += 1;
                let e = self.or()?;
                if !self.eat(&Tok::RParen) {
                    return Err("expected `)` in a condition".to_string());
                }
                Ok(e)
            }
            other => Err(format!("bad expression at {other:?}")),
        }
    }
}

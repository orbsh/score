//! Parser: line-grouped tokens into a graph.
//!
//! The chain state is `(head, current)`: a line starting with `->` continues `current`,
//! `-->` and `-[expr]->` restart from `head`.
//!
//! `def` bodies are parsed into their own graph and inlined at the call site, so the
//! graph that leaves this module has no subgraphs in it.

use std::collections::BTreeMap;

use crate::ast::{Arg, BinOp, Edge, Expr, Graph, Guard, Node};
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

    let mut defs: BTreeMap<String, Def> = BTreeMap::new();
    let mut consts: BTreeMap<String, Value> = BTreeMap::new();
    parse_lines(&lines, &mut defs, &mut consts, None)
}

fn parse_lines(
    lines: &[Vec<Tok>],
    defs: &mut BTreeMap<String, Def>,
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

    let mut i = 0usize;
    while i < lines.len() {
        let line = &lines[i];
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
                defs.insert(
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
            Tok::Ident(name) => match line.get(1) {
                Some(Tok::Eq) => {
                    let (step, used) = parse_step(
                        &mut g,
                        &line[2..],
                        consts,
                        defs,
                        &mut auto,
                        Some(name.clone()),
                    )?;
                    if used != line[2..].len() {
                        return Err(format!("trailing tokens after the call to `{name}`"));
                    }
                    head = Some((step.name.clone(), step.out_port.clone()));
                    current = head.clone();
                }
                Some(Tok::LParen) | Some(Tok::Lt) => {
                    let (step, used) = parse_step(&mut g, line, consts, defs, &mut auto, None)?;
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
        i += 1;
    }
    Ok(g)
}

fn apply_arrow(
    g: &mut Graph,
    toks: &[Tok],
    head: &Option<(String, String)>,
    current: &mut Option<(String, String)>,
    consts: &BTreeMap<String, Value>,
    defs: &BTreeMap<String, Def>,
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
                Guard::Always,
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
                Guard::Always,
            )?);
        }
        Some(Tok::CondOpen) => {
            let close = toks
                .iter()
                .position(|t| *t == Tok::CondClose)
                .ok_or("condition is not closed with `]->`")?;
            let guard = if toks[1..close] == [Tok::Star] {
                Guard::Fallback
            } else {
                Guard::When(parse_expr(&toks[1..close], consts)?)
            };
            let start = head.clone().ok_or("a conditional edge has no head")?;
            *current = Some(run_steps(
                g,
                &toks[close + 1..],
                start,
                consts,
                defs,
                auto,
                guard,
            )?);
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

fn run_steps(
    g: &mut Graph,
    toks: &[Tok],
    start: (String, String),
    consts: &BTreeMap<String, Value>,
    defs: &BTreeMap<String, Def>,
    auto: &mut usize,
    first_guard: Guard,
) -> Result<(String, String), String> {
    if toks.is_empty() {
        return Err("an arrow needs a step after it".to_string());
    }
    let mut src = start;
    let mut rest = toks;
    let mut first = true;
    loop {
        let (step, used) = parse_step(g, rest, consts, defs, auto, None)?;
        if !step.inlined {
            let in_port = step.in_port.clone().ok_or_else(|| {
                format!(
                    "`{}` needs an explicit entry port: write `<in|out>`",
                    step.name
                )
            })?;
            let guard = if first {
                first_guard.clone()
            } else {
                Guard::Always
            };
            g.edges.push(Edge {
                from: src.clone(),
                to: (step.name.clone(), in_port),
                guard,
            });
        }
        src = (step.name.clone(), step.out_port.clone());
        rest = &rest[used..];
        first = false;
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
    defs: &BTreeMap<String, Def>,
    auto: &mut usize,
    hint: Option<String>,
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

    if let Some(def) = defs.get(&op) {
        let bind =
            binding.ok_or_else(|| format!("a def call needs a name: write `n = {op}(...)`"))?;
        if spec_in.is_some() {
            return Err(format!(
                "`{op}` takes its inputs as arguments; chaining into a def is not implemented"
            ));
        }
        let out_port = inline_def(g, def, &op, &bind, &args, spec_out.as_deref())?;
        return Ok((
            Step {
                name: bind,
                out_port,
                in_port: None,
                inlined: true,
            },
            i,
        ));
    }

    let sig = ops::signature(&op).ok_or_else(|| format!("unknown operator or def `{op}`"))?;
    let name = match binding {
        Some(n) => n,
        None => {
            *auto += 1;
            format!("{op}#{}", *auto)
        }
    };
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
        let arg = args
            .get(p)
            .ok_or_else(|| format!("`{op_name}` needs `{p}: {}`", ty.name()))?;
        let got = match arg {
            Arg::Lit(v) => type_of(v),
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

    // Param sources: a literal becomes a lit node, a reference is wired directly.
    let mut resolved: BTreeMap<String, (String, String)> = BTreeMap::new();
    for (p, _) in &def.params {
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
        });
    }

    let out_node = def
        .body
        .node(&out_name)
        .ok_or_else(|| format!("`{op_name}` never binds its output `{out_name}`"))?;
    Ok(out_node.out_port.clone())
}

type DefSig = (String, Vec<(String, Ty)>, Vec<(String, Ty)>);

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

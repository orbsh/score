//! Parser: line-grouped tokens into a graph.
//!
//! The chain state is `(head, current)`: a line starting with `->` continues `current`,
//! `-->` and `-[expr]->` restart from `head`.

use std::collections::BTreeMap;

use crate::ast::{Arg, BinOp, Edge, Expr, Graph, Guard, Node};
use crate::lexer::{Tok, lex};
use crate::ops;
use crate::value::Value;

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

    let mut g = Graph::default();
    let mut consts: BTreeMap<String, Value> = BTreeMap::new();
    let mut head: Option<(String, String)> = None;
    let mut current: Option<(String, String)> = None;
    let mut auto = 0usize;

    for line in lines {
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
            Tok::Def => return Err("`def` is not implemented in this slice".to_string()),
            Tok::LBrace | Tok::RBrace => {
                return Err("unexpected brace at the top level".to_string());
            }
            Tok::Arrow | Tok::Branch | Tok::CondOpen => {
                apply_arrow(&mut g, &line, &head, &mut current, &consts, &mut auto)?;
            }
            Tok::Ident(name) => match line.get(1) {
                Some(Tok::Eq) => {
                    let (step, used) =
                        parse_step(&mut g, &line[2..], &consts, &mut auto, Some(name.clone()))?;
                    if used != line[2..].len() {
                        return Err(format!("trailing tokens after the call to `{name}`"));
                    }
                    head = Some((step.name.clone(), step.out_port.clone()));
                    current = head.clone();
                }
                Some(Tok::LParen) | Some(Tok::Lt) => {
                    let (step, used) = parse_step(&mut g, &line, &consts, &mut auto, None)?;
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
                    apply_arrow(&mut g, &line[1..], &head, &mut current, &consts, &mut auto)?;
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
    }
    Ok(g)
}

fn apply_arrow(
    g: &mut Graph,
    toks: &[Tok],
    head: &Option<(String, String)>,
    current: &mut Option<(String, String)>,
    consts: &BTreeMap<String, Value>,
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
}

fn run_steps(
    g: &mut Graph,
    toks: &[Tok],
    start: (String, String),
    consts: &BTreeMap<String, Value>,
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
        let (step, used) = parse_step(g, rest, consts, auto, None)?;
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
    let sig = ops::signature(&op).ok_or_else(|| format!("unknown operator `{op}`"))?;

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
        },
        i,
    ))
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

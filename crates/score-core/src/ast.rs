//! The graph a program parses into: nodes, ports, edges, guards.

use std::collections::BTreeMap;

use crate::value::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    Lit(Value),
    Ref(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Lit(Value),
    /// A bare name inside a condition: a port of the edge's source node.
    Local(String),
    Bin(BinOp, Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Guard {
    Always,
    When(Expr),
    Fallback,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub name: String,
    pub op: String,
    pub args: BTreeMap<String, Arg>,
    /// The exit port this node's value is stored under (its use site decides it).
    pub out_port: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    pub from: (String, String),
    pub to: (String, String),
    pub guard: Guard,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

impl Graph {
    pub fn node(&self, name: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.name == name)
    }

    /// FNV-1a over a canonical dump: nodes in order with their ports and
    /// arguments, then edges with their guards. Two graphs that differ in any
    /// of it hash apart; two that differ only in layout hash the same.
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for n in &self.nodes {
            field(&mut h, "n");
            field(&mut h, &n.name);
            field(&mut h, &n.op);
            field(&mut h, &n.out_port);
            for (k, a) in &n.args {
                field(&mut h, "a");
                field(&mut h, k);
                field(&mut h, &format!("{a:?}"));
            }
        }
        for e in &self.edges {
            field(&mut h, "e");
            field(&mut h, &e.from.0);
            field(&mut h, &e.from.1);
            field(&mut h, &e.to.0);
            field(&mut h, &e.to.1);
            field(&mut h, &format!("{:?}", e.guard));
        }
        h
    }
}

fn field(h: &mut u64, s: &str) {
    for b in s.as_bytes() {
        *h ^= u64::from(*b);
        *h = h.wrapping_mul(0x100_0000_01b3);
    }
    *h ^= 0x1f;
    *h = h.wrapping_mul(0x100_0000_01b3);
}

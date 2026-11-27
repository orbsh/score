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
}

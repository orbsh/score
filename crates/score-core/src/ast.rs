//! The graph a program parses into: nodes, ports, edges, guards.

use std::collections::BTreeMap;

use crate::value::{Ty, Value};

#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    Lit(Value),
    Ref(String),
    /// A body to run per element: the `f` argument of `map` / `fold`.
    Sub(Box<SubGraph>),
}

/// A def body kept as a template: `inputs` are the param nodes the runtime
/// seeds for each call, `output` is the node and port that carry the result.
#[derive(Debug, Clone, PartialEq)]
pub struct SubGraph {
    pub graph: Graph,
    /// The param nodes the runtime seeds, with the port they are read from.
    pub inputs: Vec<(String, String)>,
    pub output: (String, String),
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
    /// A back edge: it may carry a value more than once, which is what makes a
    /// ring turn. `budget` caps the deliveries; `None` means unbounded.
    pub ring: bool,
    pub budget: Option<u32>,
}

/// A constructor declared by `data`: the variant name is the identity, the
/// fields are its payload, and the name doubles as an output port.
#[derive(Debug, Clone, PartialEq)]
pub struct Ctor {
    pub sum: String,
    pub fields: Vec<(String, Ty)>,
}

/// A constructor set of an `enum`, keyed by variant name: the variant name is
/// the identity, the fields are its payload, and the name doubles as an output
/// port. See `Ctor`.
pub type Attrs = BTreeMap<String, Option<String>>;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    /// Constructor set of each `enum`, keyed by variant name.
    pub ctors: BTreeMap<String, Ctor>,
    /// Fields of each `struct`; the struct name is also its constructor.
    pub structs: BTreeMap<String, Vec<(String, Ty)>>,
    /// `@doc` / `@tag` written above a node, keyed by the node's name.
    pub node_attrs: BTreeMap<String, Attrs>,
    /// The same, written above a declaration and keyed by its name.
    pub decl_attrs: BTreeMap<String, Attrs>,
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
                match a {
                    Arg::Sub(sub) => {
                        field(&mut h, &format!("{:016x}", sub.graph.fingerprint()));
                        for input in &sub.inputs {
                            field(&mut h, &input.0);
                            field(&mut h, &input.1);
                        }
                        field(&mut h, &sub.output.0);
                        field(&mut h, &sub.output.1);
                    }
                    other => field(&mut h, &format!("{other:?}")),
                }
            }
        }
        for e in &self.edges {
            field(&mut h, "e");
            field(&mut h, &e.from.0);
            field(&mut h, &e.from.1);
            field(&mut h, &e.to.0);
            field(&mut h, &e.to.1);
            field(&mut h, &format!("{:?}", e.guard));
            field(&mut h, if e.ring { "ring" } else { "edge" });
            field(&mut h, &format!("{:?}", e.budget));
        }
        for (name, ctor) in &self.ctors {
            field(&mut h, "d");
            field(&mut h, name);
            field(&mut h, &ctor.sum);
            for (f, t) in &ctor.fields {
                field(&mut h, f);
                field(&mut h, t.name());
            }
        }
        for (name, fields) in &self.structs {
            field(&mut h, "s");
            field(&mut h, name);
            for (f, t) in fields {
                field(&mut h, f);
                field(&mut h, t.name());
            }
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

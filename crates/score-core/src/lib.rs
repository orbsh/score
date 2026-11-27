//! score-core: a first vertical slice of the language.

pub mod ast;
pub mod exec;
pub mod lexer;
pub mod ops;
pub mod parser;
pub mod validate;
pub mod value;

pub use ast::{Arg, Edge, Expr, Graph, Guard, Node};
pub use exec::{Run, run};
pub use parser::parse;
pub use validate::validate;
pub use value::{Ty, Value};

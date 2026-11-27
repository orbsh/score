use score_core::{Graph, Value, exec, parser, validate};

fn build(src: &str) -> Graph {
    let g = parser::parse(src).expect("parse");
    validate::validate(&g).expect("validate");
    g
}

#[test]
fn sum_runs() {
    let g = build(include_str!("../../../examples/sum.score"));
    let run = exec::run(&g, false).expect("run");
    let total = run.values.get(&("total".to_string(), "out".to_string()));
    assert_eq!(total, Some(&Value::Int(5)));
}

#[test]
fn chain_passes_through_ports() {
    let g = build(include_str!("../../../examples/chain.score"));
    let run = exec::run(&g, false).expect("run");
    assert!(
        run.order.iter().any(|n| n.starts_with("add#")),
        "order: {:?}",
        run.order
    );
}

#[test]
fn branch_takes_exactly_one_arm() {
    let g = build(include_str!("../../../examples/branch.score"));
    let run = exec::run(&g, false).expect("run");
    let prints = run.order.iter().filter(|n| n.starts_with("print#")).count();
    assert_eq!(prints, 1, "order: {:?}", run.order);
    assert_eq!(run.skipped.len(), 1, "skipped: {:?}", run.skipped);
}

#[test]
fn newlines_are_layout() {
    let one = "s = lit(value: 1)\ns -> add<a|out>(b: 1) -> print\n";
    let split = "s = lit(value: 1)\ns -> add<a|out>(b: 1)\n->print\n";
    let a = parser::parse(one).expect("one");
    let b = parser::parse(split).expect("split");
    assert_eq!(a.nodes.len(), b.nodes.len());
    assert_eq!(a.edges.len(), b.edges.len());
}

#[test]
fn def_inlines_at_the_call_site() {
    let g = build(include_str!("../../../examples/def.score"));
    let run = exec::run(&g, false).expect("run");
    assert_eq!(
        run.values.get(&("m".to_string(), "out".to_string())),
        Some(&Value::Int(42))
    );
}

#[test]
fn def_accepts_a_literal_argument() {
    let src = "def double(x: Int) -> (result) {\n  result = add(a: x, b: x)\n}\nm = double(x: 5)\n";
    let g = build(src);
    let run = exec::run(&g, false).expect("run");
    assert_eq!(
        run.values.get(&("m".to_string(), "out".to_string())),
        Some(&Value::Int(10))
    );
}

#[test]
fn def_with_a_missing_argument_is_rejected() {
    let src = "def double(x: Int) -> (result) {\n  result = add(a: x, b: x)\n}\nn = lit(value: 1)\nm = double()\n";
    let e = parser::parse(src).expect_err("must be invalid");
    assert!(e.contains("needs `x"), "{e}");
}

#[test]
fn both_argument_and_edge_is_rejected() {
    let src = "a = lit(value: 1)\nm = add(a: a, b: 2)\nm -> add<a|out>(a: 3)\n";
    let g = parser::parse(src).expect("parse");
    let e = validate::validate(&g).expect_err("must be invalid");
    assert!(e.contains("both an argument and an edge"), "{e}");
}

#[test]
fn branch_without_fallback_is_rejected() {
    let src = "n = lit(value: 5)\nn -[out > 5]-> print\n";
    let g = parser::parse(src).expect("parse");
    let e = validate::validate(&g).expect_err("must be invalid");
    assert!(e.contains("no `[*]` arm"), "{e}");
}

#[test]
fn condition_names_must_be_source_ports() {
    let src = "n = lit(value: 5)\nn -[value > 5]-> print\nn -[*]-> print\n";
    let g = parser::parse(src).expect("parse");
    let e = validate::validate(&g).expect_err("must be invalid");
    assert!(e.contains("not a port of"), "{e}");
}

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
fn lists_flow_through_operators() {
    let g = build(include_str!("../../../examples/list.score"));
    let run = exec::run(&g, false).expect("run");
    let xs = run.values.get(&("xs".to_string(), "out".to_string()));
    assert_eq!(
        xs,
        Some(&Value::List(vec![
            Value::Int(0),
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
            Value::Int(4)
        ]))
    );
    assert_eq!(
        run.values.get(&("total".to_string(), "out".to_string())),
        Some(&Value::Int(10))
    );
    assert_eq!(
        run.values.get(&("text".to_string(), "out".to_string())),
        Some(&Value::Text("0-1-2-3-4".to_string()))
    );
}

#[test]
fn a_list_survives_the_journal() {
    let g = build(include_str!("../../../examples/list.score"));
    let run = exec::run(&g, false).expect("run");
    let text = score_core::journal::write(g.fingerprint(), &run.entries);
    let back = score_core::journal::read(&text).expect("read");
    let xs = back
        .entries
        .iter()
        .find(|e| e.node == "xs")
        .map(|e| e.value.clone());
    assert_eq!(
        xs,
        Some(Value::List((0..5).map(Value::Int).collect::<Vec<_>>()))
    );
}

#[test]
fn journal_round_trips() {
    let g = build(include_str!("../../../examples/sum.score"));
    let run = exec::run(&g, false).expect("run");
    let text = score_core::journal::write(g.fingerprint(), &run.entries);
    let back = score_core::journal::read(&text).expect("read");
    assert_eq!(back.graph, Some(g.fingerprint()));
    assert_eq!(back.entries.len(), run.entries.len());
    assert_eq!(back.entries[2].value, Value::Int(5));
    assert_eq!(back.entries[2].node, "total");
}

#[test]
fn fingerprint_ignores_layout_but_not_content() {
    let a = parser::parse("s = lit(value: 1)\ns -> add<a|out>(b: 2)\n").expect("a");
    let b = parser::parse("s = lit(value: 1)\ns\n -> add<a|out>(b: 2)\n").expect("b");
    assert_eq!(a.fingerprint(), b.fingerprint());
    let c = parser::parse("s = lit(value: 2)\ns -> add<a|out>(b: 2)\n").expect("c");
    assert_ne!(a.fingerprint(), c.fingerprint());
}

#[test]
fn resume_finishes_where_the_journal_stopped() {
    let g = build(include_str!("../../../examples/sum.score"));
    let first = exec::run(&g, false).expect("run");
    let mut seed: std::collections::BTreeMap<(String, String), Value> =
        std::collections::BTreeMap::new();
    for e in &first.entries[..3] {
        seed.insert((e.node.clone(), e.port.clone()), e.value.clone());
    }
    let again = exec::run_from(&g, false, &seed, 3).expect("resume");
    assert_eq!(again.entries.len(), 1);
    assert_eq!(again.entries[0].node, "print#1");
    assert_eq!(again.entries[0].seq, 4);
    assert_eq!(
        again.values.get(&("total".to_string(), "out".to_string())),
        Some(&Value::Int(5))
    );
}

#[test]
fn resume_rejects_a_value_the_graph_no_longer_has() {
    let g = build(include_str!("../../../examples/sum.score"));
    let mut seed: std::collections::BTreeMap<(String, String), Value> =
        std::collections::BTreeMap::new();
    seed.insert(("ghost".to_string(), "out".to_string()), Value::Int(1));
    let e = exec::run_from(&g, false, &seed, 0).expect_err("must fail");
    assert!(e.contains("ghost"), "{e}");
}

#[test]
fn journal_has_one_entry_per_firing() {
    let g = build(include_str!("../../../examples/sum.score"));
    let run = exec::run(&g, false).expect("run");
    assert_eq!(run.entries.len(), 4);
    assert_eq!(run.entries[2].seq, 3);
    assert_eq!(run.entries[2].node, "total");
    assert_eq!(run.entries[2].value, Value::Int(5));
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

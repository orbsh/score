use std::collections::BTreeMap;

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
fn a_graph_renders_back_to_text() {
    let g = build(include_str!("../../../examples/enum.score"));
    let text = score_core::render::graph_text(&g);
    assert!(
        text.contains("enum Shape = Circle { r: Int } | Rect { w: Int, h: Int }"),
        "{text}"
    );
    assert!(text.contains("c.Circle -> print#1.value"), "{text}");

    let ring = build(include_str!("../../../examples/ring.score"));
    assert!(
        score_core::render::graph_text(&ring).contains("acc.out -> acc.a budget 5"),
        "the ring edge keeps its budget"
    );
}

#[test]
fn a_resumed_ring_reads_its_budget_from_the_journal() {
    use std::collections::BTreeMap;

    let g = build(include_str!("../../../examples/ring.score"));
    let full = exec::run(&g, false).expect("run");
    let text = score_core::journal::write(g.fingerprint(), &full.entries);
    let back = score_core::journal::read(&text).expect("read");

    // Stop the journal in the middle of the ring, as a crash would: after the
    // first turn, with one budget line written.
    let cut = 4;
    let prefix: Vec<_> = back
        .entries
        .iter()
        .filter(|e| e.seq <= cut)
        .cloned()
        .collect();
    let mut seed = BTreeMap::new();
    let mut budgets = BTreeMap::new();
    for e in &prefix {
        match e.kind {
            exec::Kind::Budget => {
                if let Value::Int(left) = e.value {
                    budgets.insert((e.node.clone(), e.port.clone()), left.max(0) as u32);
                }
            }
            exec::Kind::Fire => {
                seed.insert((e.node.clone(), e.port.clone()), e.value.clone());
            }
        }
    }

    let again = exec::run_from(&g, false, &seed, &budgets, cut).expect("resume");

    // The ring does not turn again: every node that fired is seeded and seeded
    // nodes never re-fire. What the journal has to carry is the budget, so a
    // resumed run must not hand the ring a fresh one.
    assert!(
        !again
            .entries
            .iter()
            .any(|e| e.kind == exec::Kind::Fire && e.node == "acc")
    );
    let spent = again
        .entries
        .iter()
        .find(|e| e.kind == exec::Kind::Budget)
        .map(|e| e.value.clone());
    assert_eq!(
        spent,
        Some(Value::Int(3)),
        "4 left in the journal, minus one"
    );
}

#[test]
fn attributes_are_tags_on_declarations_and_nodes() {
    let g = build(include_str!("../../../examples/attrs.score"));
    assert_eq!(
        g.decl_attrs.get("double").and_then(|a| a.get("doc")),
        Some(&Some("把参数加倍".to_string()))
    );
    assert_eq!(
        g.node_attrs.get("n").and_then(|a| a.get("tag")),
        Some(&Some("seed".to_string()))
    );
    let run = exec::run(&g, false).expect("run");
    assert!(run.entries.iter().any(|e| e.node == "double#1"));
}

#[test]
fn an_unknown_attribute_is_rejected() {
    let e = parser::parse("@log(off)\nn = lit(value: 1)\n").expect_err("parse");
    assert!(e.contains("unknown attribute `@log`"), "{e}");
}

#[test]
fn enums_and_structs_build_tagged_values() {
    let g = build(include_str!("../../../examples/enum.score"));
    let run = exec::run(&g, false).expect("run");
    match run.values.get(&("c".to_string(), "Circle".to_string())) {
        Some(Value::Variant { tag, fields }) => {
            assert_eq!(tag, "Circle");
            assert_eq!(fields.get("r"), Some(&Value::Int(3)));
        }
        other => panic!("expected a Circle, got {other:?}"),
    }
    match run.values.get(&("p".to_string(), "out".to_string())) {
        Some(Value::Record(fields)) => {
            assert_eq!(fields.get("age"), Some(&Value::Int(36)));
        }
        other => panic!("expected a record, got {other:?}"),
    }
}

#[test]
fn a_variant_survives_the_journal() {
    let g = build(include_str!("../../../examples/enum.score"));
    let run = exec::run(&g, false).expect("run");
    let text = score_core::journal::write(g.fingerprint(), &run.entries);
    let back = score_core::journal::read(&text).expect("read");
    let c = back
        .entries
        .iter()
        .find(|e| e.node == "c")
        .map(|e| e.value.clone());
    match c {
        Some(Value::Variant { tag, fields }) => {
            assert_eq!(tag, "Circle");
            assert_eq!(fields.get("r"), Some(&Value::Int(3)));
        }
        other => panic!("expected a Circle, got {other:?}"),
    }
}

#[test]
fn records_hold_named_fields() {
    let g = build(include_str!("../../../examples/record.score"));
    let run = exec::run(&g, false).expect("run");
    assert_eq!(
        run.values.get(&("who".to_string(), "out".to_string())),
        Some(&Value::Text("ada".to_string()))
    );
    assert_eq!(
        run.values.get(&("age".to_string(), "out".to_string())),
        Some(&Value::Int(36))
    );
}

#[test]
fn a_record_survives_the_journal() {
    let g = build(include_str!("../../../examples/record.score"));
    let run = exec::run(&g, false).expect("run");
    let text = score_core::journal::write(g.fingerprint(), &run.entries);
    let back = score_core::journal::read(&text).expect("read");
    let r = back
        .entries
        .iter()
        .find(|e| e.node == "r")
        .map(|e| e.value.clone());
    match r {
        Some(Value::Record(fields)) => {
            assert_eq!(fields.get("age"), Some(&Value::Int(36)));
            assert_eq!(fields.get("name"), Some(&Value::Text("ada".to_string())));
        }
        other => panic!("expected a record, got {other:?}"),
    }
}

#[test]
fn a_chain_can_enter_a_def() {
    let g = build(include_str!("../../../examples/chain_def.score"));
    let run = exec::run(&g, false).expect("run");
    let got = run
        .entries
        .iter()
        .find(|e| e.node == "double#1")
        .map(|e| e.value.clone());
    assert_eq!(got, Some(Value::Int(42)));
}

#[test]
fn filter_and_predicates_run_a_def() {
    let g = build(include_str!("../../../examples/filter.score"));
    let run = exec::run(&g, false).expect("run");
    assert_eq!(
        run.values.get(&("ys".to_string(), "out".to_string())),
        Some(&Value::List(vec![
            Value::Int(3),
            Value::Int(4),
            Value::Int(5)
        ]))
    );
    assert_eq!(
        run.values.get(&("some".to_string(), "out".to_string())),
        Some(&Value::Bool(true))
    );
    assert_eq!(
        run.values.get(&("every".to_string(), "out".to_string())),
        Some(&Value::Bool(false))
    );
}

#[test]
fn a_ring_turns_within_its_budget() {
    let g = build(include_str!("../../../examples/ring.score"));
    let run = exec::run(&g, false).expect("run");
    assert_eq!(
        run.values.get(&("acc".to_string(), "out".to_string())),
        Some(&Value::Int(6))
    );
    assert_eq!(run.order.iter().filter(|n| *n == "acc").count(), 6);
}

#[test]
fn a_ring_without_a_budget_is_rejected() {
    let src = "zero = lit(value: 0)\none = lit(value: 1)\nacc = add(b: one)\nzero -> acc<a|>\nacc -> acc<a|>\n";
    let g = parser::parse(src).expect("parse");
    let e = validate::validate(&g).expect_err("must be invalid");
    assert!(e.contains("ring"), "{e}");
}

#[test]
fn map_runs_a_def_per_element() {
    let g = build(include_str!("../../../examples/containers.score"));
    let run = exec::run(&g, false).expect("run");
    assert_eq!(
        run.values.get(&("ys".to_string(), "out".to_string())),
        Some(&Value::List((1..=5).map(Value::Int).collect::<Vec<_>>()))
    );
    assert_eq!(
        run.values.get(&("total".to_string(), "out".to_string())),
        Some(&Value::Int(15))
    );
}

#[test]
fn map_needs_a_def_of_the_right_arity() {
    let src = "def inc(x: Int) -> (result) {\n  result = add(a: x, b: 1)\n}\nxs = range(n: 2)\nys = fold(f: inc, acc: 0, xs: xs)\n";
    let e = parser::parse(src).expect_err("must fail");
    assert!(e.contains("2 parameter"), "{e}");
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
    let again = exec::run_from(&g, false, &seed, &BTreeMap::new(), 3).expect("resume");
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
    let e = exec::run_from(&g, false, &seed, &BTreeMap::new(), 0).expect_err("must fail");
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

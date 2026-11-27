use std::collections::BTreeMap;
use std::process::ExitCode;

use score_core::{Value, exec, journal, parser, validate};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut trace = false;
    let mut journal_out: Option<String> = None;
    let mut resume: Option<String> = None;
    let mut positional: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--trace" => trace = true,
            "--journal" => match take_path(&args, &mut i, "--journal") {
                Some(p) => journal_out = Some(p),
                None => return ExitCode::from(2),
            },
            "--resume" => match take_path(&args, &mut i, "--resume") {
                Some(p) => resume = Some(p),
                None => return ExitCode::from(2),
            },
            other => positional.push(other.to_string()),
        }
        i += 1;
    }
    if positional.len() < 2 {
        eprintln!(
            "usage: score <validate|run> <file> [--trace] [--journal <path>] [--resume <journal>]"
        );
        return ExitCode::from(2);
    }
    let cmd = &positional[0];
    let path = &positional[1];

    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let graph = match parser::parse(&src) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("parse error: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = validate::validate(&graph) {
        eprintln!("invalid: {e}");
        return ExitCode::FAILURE;
    }

    match cmd.as_str() {
        "validate" => {
            println!(
                "ok ({} nodes, {} edges)",
                graph.nodes.len(),
                graph.edges.len()
            );
            ExitCode::SUCCESS
        }
        "run" => {
            let mut seed: BTreeMap<(String, String), Value> = BTreeMap::new();
            let mut done: Vec<exec::Entry> = Vec::new();
            let mut seq_base = 0usize;
            if let Some(path) = &resume {
                let text = match std::fs::read_to_string(path) {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!("cannot read {path}: {e}");
                        return ExitCode::FAILURE;
                    }
                };
                let entries = match journal::read(&text) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("bad journal {path}: {e}");
                        return ExitCode::FAILURE;
                    }
                };
                for e in &entries {
                    match graph.node(&e.node) {
                        Some(n) if n.op == e.op => {}
                        Some(n) => {
                            eprintln!(
                                "the journal has `{}` as `{}`, but this graph has `{}`",
                                e.node, e.op, n.op
                            );
                            return ExitCode::FAILURE;
                        }
                        None => {
                            eprintln!(
                                "the journal has `{}`, which is not a node of this graph",
                                e.node
                            );
                            return ExitCode::FAILURE;
                        }
                    }
                    seed.insert((e.node.clone(), e.port.clone()), e.value.clone());
                    seq_base = seq_base.max(e.seq);
                }
                println!("resume: {} entries already done", entries.len());
                done = entries;
            }

            let run = match exec::run_from(&graph, trace, &seed, seq_base) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("run failed: {e}");
                    return ExitCode::FAILURE;
                }
            };

            let mut all = done;
            all.extend(run.entries.iter().cloned());
            if let Some(path) = &journal_out
                && let Err(e) = std::fs::write(path, journal::write(&all))
            {
                eprintln!("cannot write {path}: {e}");
                return ExitCode::FAILURE;
            }

            println!(
                "ok ({} steps, {} skipped)",
                run.order.len(),
                run.skipped.len()
            );
            if let Some(path) = &journal_out {
                println!("journal: {} entries -> {path}", all.len());
            }
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("unknown command `{other}`");
            ExitCode::from(2)
        }
    }
}

fn take_path(args: &[String], i: &mut usize, flag: &str) -> Option<String> {
    *i += 1;
    match args.get(*i) {
        Some(p) => Some(p.clone()),
        None => {
            eprintln!("{flag} needs a path");
            None
        }
    }
}

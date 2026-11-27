use std::process::ExitCode;

use score_core::{exec, parser, validate};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: score <validate|run> <file> [--trace]");
        return ExitCode::from(2);
    }
    let cmd = &args[0];
    let path = &args[1];
    let trace = args.iter().any(|a| a == "--trace");

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
        "run" => match exec::run(&graph, trace) {
            Ok(run) => {
                println!(
                    "ok ({} steps, {} skipped)",
                    run.order.len(),
                    run.skipped.len()
                );
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("run failed: {e}");
                ExitCode::FAILURE
            }
        },
        other => {
            eprintln!("unknown command `{other}`");
            ExitCode::from(2)
        }
    }
}

//! Instant first page lab: the Rust preview host prototype.
//!
//!   host index <project> [--saved <file>] [--out <file>] [--include <glob>]…
//!       [--evaluator <evaluate.mjs>] [--node <node>] [--check walk|dirs]
//!   host basic <project>
//!   host serve      (the compiler host; see compile.rs)
//!
//! `index` prints its timings as JSON, with `process_ms` from process start;
//! `--out` writes the index for comparison with the shipping one.

mod compile;
mod index;

use std::path::PathBuf;
use std::time::Instant;

use rspack::builder::Builder as _;
use rspack_core::Compiler;

fn option(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|arg| arg == name).and_then(|at| args.get(at + 1)).cloned()
}

fn options(args: &[String], name: &str) -> Vec<String> {
    args.windows(2).filter(|pair| pair[0] == name).map(|pair| pair[1].clone()).collect()
}

fn main() {
    let process = Instant::now();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("index") => {
            let root = PathBuf::from(args.get(1).expect("usage: host index <project>"));
            let lab = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
            let mut include = options(&args, "--include");
            if include.is_empty() {
                include = vec!["**/*.workbench.ts".into(), "**/*.workbench.tsx".into()];
            }
            let options = index::Options {
                root: std::fs::canonicalize(&root).unwrap_or(root),
                include,
                saved: option(&args, "--saved").map(PathBuf::from),
                evaluator: option(&args, "--evaluator").map(PathBuf::from).unwrap_or_else(|| lab.join("evaluate.mjs")),
                node: option(&args, "--node").map(PathBuf::from).unwrap_or_else(|| "node".into()),
                check: if option(&args, "--check").as_deref() == Some("dirs") { index::Check::Dirs } else { index::Check::Walk },
            };
            let (index, timings) = index::index(&options);
            let process_ms = process.elapsed().as_secs_f64() * 1000.0;
            if let Some(out) = option(&args, "--out") {
                std::fs::write(out, &index).expect("write index");
            }
            let mut report = serde_json::to_value(&timings).expect("timings");
            report["process_ms"] = serde_json::json!(process_ms);
            report["bytes"] = serde_json::json!(index.len());
            println!("{report}");
            // Skip freeing everything on the way out.
            std::process::exit(0);
        }
        Some("serve") => {
            let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("runtime");
            runtime.block_on(rspack_tasks::within_compiler_context(std::sync::Arc::new(rspack_tasks::CompilerContext::new()), compile::serve()));
        }
        Some("basic") => {
            let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("runtime");
            runtime.block_on(basic(args.get(1).expect("usage: host basic <project>")));
        }
        _ => {
            eprintln!("usage: host index <project> … | host basic <project>");
            std::process::exit(2);
        }
    }
}

/// C1: the smallest build through the crates.
async fn basic(context: &str) {
    let mut compiler = Compiler::builder().context(context).entry("main", "./src/index.js").build().expect("compiler options");
    let started = Instant::now();
    compiler.build().await.expect("build");
    let assets: Vec<String> = compiler.compilation.assets().keys().cloned().collect();
    println!("built {} assets in {:?}: {:?}", assets.len(), started.elapsed(), assets);
}

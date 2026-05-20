use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};

use cs_test::report::{render_json, render_markdown};
use cs_test::results::{build_results_file, write_results_file};
use cs_test::runner::run_file;

#[derive(Parser)]
#[command(name = "cs-test", version, about = "ERC-7730 test runner using the clear-signing engine")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Run {
        file: PathBuf,
        #[arg(long)]
        case: Option<String>,
        #[arg(long)]
        json: bool,
        /// Emit registry-compatible `results.json` to <path> instead of the
        /// markdown report. Per the registry contract, exit code stays 0 even
        /// when individual cases fail — only runner-level failures (unreadable
        /// input, unwritable output) are non-zero.
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Run { file, case, json, output } => match dispatch(file, case, json, output) {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::from(1),
            Err(e) => {
                eprintln!("error: {e:#}");
                ExitCode::from(2)
            }
        },
    }
}

fn dispatch(
    file: PathBuf,
    case: Option<String>,
    json: bool,
    output: Option<PathBuf>,
) -> Result<bool> {
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    let results = runtime.block_on(run_file(&file, case.as_deref()))?;
    if let Some(output_path) = output {
        // Registry mode: well-formed `results.json` is the contract. Per-case
        // pass/fail/error/skipped does not influence the process exit code.
        let results_file = build_results_file(&results);
        write_results_file(&output_path, &results_file)?;
        return Ok(true);
    }
    let all_passed = results.iter().all(|r| r.passed && r.error.is_none());
    if json {
        println!("{}", render_json(&results));
    } else {
        println!("{}", render_markdown(&results));
    }
    Ok(all_passed)
}

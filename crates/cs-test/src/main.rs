use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};

use cs_test::report::{render_json, render_markdown};
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
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Run { file, case, json } => match dispatch(file, case, json) {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::from(1),
            Err(e) => {
                eprintln!("error: {e:#}");
                ExitCode::from(2)
            }
        },
    }
}

fn dispatch(file: PathBuf, case: Option<String>, json: bool) -> Result<bool> {
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    let results = runtime.block_on(run_file(&file, case.as_deref()))?;
    let all_passed = results.iter().all(|r| r.passed);
    if json {
        println!("{}", render_json(&results));
    } else {
        println!("{}", render_markdown(&results));
    }
    Ok(all_passed)
}

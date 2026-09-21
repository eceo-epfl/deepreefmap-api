//! Write the published contract artefacts, or check the checked-in copies are current.
//!
//! Needs no database and no running server.

use std::path::Path;
use std::process::ExitCode;

use deepreefmap_api::contract::export::{self, CONTRACT_DIR};

const USAGE: &str = "usage: export-contract [--check] [--dir <path>]";

fn main() -> ExitCode {
    let mut check = false;
    let mut dir = CONTRACT_DIR.to_string();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--check" => check = true,
            "--dir" => {
                if let Some(path) = args.next() {
                    dir = path;
                } else {
                    eprintln!("--dir needs a path\n{USAGE}");
                    return ExitCode::FAILURE;
                }
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("unknown argument {other}\n{USAGE}");
                return ExitCode::FAILURE;
            }
        }
    }

    let dir = Path::new(&dir);
    if check {
        let stale = export::check(dir);
        if stale.is_empty() {
            return ExitCode::SUCCESS;
        }
        eprintln!("{} contract artefact(s) are stale:", stale.len());
        for report in &stale {
            eprintln!("{report}");
        }
        eprintln!("run `cargo run --bin export-contract` and commit the result");
        return ExitCode::FAILURE;
    }

    match export::write_all(dir) {
        Ok(written) => {
            for path in written {
                println!("{}", dir.join(path).display());
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("failed to write into {}: {e}", dir.display());
            ExitCode::FAILURE
        }
    }
}

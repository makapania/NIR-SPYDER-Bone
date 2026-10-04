//! `spyder`: the SPYDER Bone command-line tool, built on `spyder-core`.
//!
//! spyder read     <file.asd | folder> [--json] [--recursive]
//! spyder validate <plug-in file | folder> [--json] [--user] [--pin id@version]...
//! spyder predict  <file.asd | folder> --class std|hires [--json] [--recursive]
//!                 [--plugins DIR] [--user-plugins DIR] [--pin id@version]...
//! spyder analyse  <file.asd | folder> --class std|hires [--profile radiocarbon|isotopes|zooms]
//!                 [--csv out.csv] [--json] [--sort] [--recursive] [--plugins DIR] [--user-plugins DIR] [--pin ...]
//!
//! Arguments are taken as OS strings (paths need not be UTF-8).

mod analyse_cmd;
mod plugin_opts;
mod predict_cmd;
mod read_cmd;
mod validate_cmd;

use std::ffi::OsString;
use std::process::ExitCode;

const USAGE: &str = "usage: spyder <command> ...

  read      read .asd files: acceptance, header, N2, acquisition checks B1-B8
  validate  validate plug-in files and run their goldens with the app engine
  predict   run every active model and CONS3 on .asd files
  analyse   the full pipeline: verdict, evidence, ZooMS pattern, flags, CSV

  spyder <command> --help    details of one command
  spyder --version
";

fn run(args: &[OsString]) -> Result<(), (u8, String)> {
    let cmd = args.first().and_then(|a| a.to_str());
    let rest = if args.is_empty() { &[][..] } else { &args[1..] };
    match cmd {
        Some("read") => read_cmd::run(rest),
        Some("validate") => validate_cmd::run(rest),
        Some("predict") => predict_cmd::run(rest),
        Some("analyse" | "analyze") => analyse_cmd::run(rest),
        Some("-h" | "--help" | "help") => {
            print!("{USAGE}");
            Ok(())
        }
        None if args.is_empty() => {
            print!("{USAGE}");
            Ok(())
        }
        Some("--version" | "-V") => {
            println!("spyder {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some(other) => Err((2, format!("unknown command {other:?}\n\n{USAGE}"))),
        // a non-UTF-8 command word
        None => Err((
            2,
            format!("unknown command {:?}\n\n{USAGE}", args[0].to_string_lossy()),
        )),
    }
}

fn main() -> ExitCode {
    // args_os: a non-UTF-8 path must not panic (Codex Phase 1 review, MEDIUM 5)
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err((code, msg)) => {
            if !msg.is_empty() {
                eprintln!("{msg}");
            }
            ExitCode::from(code)
        }
    }
}

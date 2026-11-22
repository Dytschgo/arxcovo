use std::env;
use std::io;
use std::process::ExitCode;

use arx_guest::{ScriptExecution, ScriptExecutor, ScriptOutcome, serve};

struct UnavailableExecutor;

impl ScriptExecutor for UnavailableExecutor {
    fn script_execution(&self) -> ScriptExecution {
        ScriptExecution::Unavailable
    }

    fn run_script(&mut self, _script: &str, _timeout_s: u32) -> ScriptOutcome {
        ScriptOutcome::Unavailable
    }
}

fn main() -> ExitCode {
    let mut args = env::args_os().skip(1);
    match (
        args.next().as_deref().and_then(|arg| arg.to_str()),
        args.next(),
    ) {
        (Some("--version"), None) => {
            println!("arx-guest {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        (Some("serve-stdio"), None) => {
            match serve(
                io::stdin().lock(),
                io::stdout().lock(),
                &mut UnavailableExecutor,
            ) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("arx-guest: {error}");
                    ExitCode::FAILURE
                }
            }
        }
        _ => {
            eprintln!("Usage: arx-guest --version | serve-stdio");
            ExitCode::from(2)
        }
    }
}

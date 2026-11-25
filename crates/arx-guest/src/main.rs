use std::env;
use std::io::{self, Write};
use std::process::ExitCode;

use arx_guest::{ScriptExecution, ScriptExecutor, ScriptOutcome, serve};

#[cfg(any(target_os = "linux", windows))]
mod platform;

#[cfg(target_os = "linux")]
const USAGE: &str = "Usage: arx-guest --version | serve-stdio | serve-virtio";
#[cfg(windows)]
const USAGE: &str = "Usage: arx-guest --version | serve-stdio | serve-service";
#[cfg(not(any(target_os = "linux", windows)))]
const USAGE: &str = "Usage: arx-guest --version | serve-stdio";

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
            let _ = writeln!(
                io::stdout().lock(),
                "arx-guest {}",
                env!("CARGO_PKG_VERSION")
            );
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
                    let _ = writeln!(io::stderr().lock(), "arx-guest: {error}");
                    ExitCode::FAILURE
                }
            }
        }
        #[cfg(target_os = "linux")]
        (Some("serve-virtio"), None) => match platform::serve_virtio() {
            Ok(()) => ExitCode::SUCCESS,
            Err(category) => {
                let _ = writeln!(io::stderr().lock(), "arx-guest: {category}");
                ExitCode::FAILURE
            }
        },
        #[cfg(target_os = "linux")]
        (Some("backup-sqlite-helper"), None) => {
            arx_guest::compose_backup::run_linux_backup_helper_main()
        }
        #[cfg(windows)]
        (Some("serve-service"), None) => match platform::serve_service() {
            Ok(()) => ExitCode::SUCCESS,
            Err(category) => {
                let _ = writeln!(io::stderr().lock(), "arx-guest: {category}");
                ExitCode::FAILURE
            }
        },
        _ => {
            let _ = writeln!(io::stderr().lock(), "{USAGE}");
            ExitCode::from(2)
        }
    }
}

//! Owner-local Windows VM qualification runner.
//!
//! This binary crate owns the private `TrialAuthority`; the arxd library and
//! the daemon cannot name it, and it never converts into `LaunchAuthority`.
//! The library provides held preflight, a fixed paused-trial adapter, and
//! a diagnostic-only evidence sink. The executable publishes incomplete
//! qualification diagnostics.
//!
//! An evidence directory serves exactly one run. Before the runner's claim
//! exists, the sink removes only its own temporary entries left by an
//! interrupted commit, so a retry in the same directory can claim it. Once a
//! claim exists the directory belongs to that run whatever its outcome. A run
//! whose diagnostics or summary could not be committed fails with `Output`
//! and a nonzero exit, and leaves no entry under the failed file's final
//! name; a retry uses a fresh empty directory.

// The portable framing and trial graph compile on every platform for tests;
// outside Windows x64 the executable itself only reports the platform.
#[cfg(any(test, all(windows, target_arch = "x86_64")))]
mod frame;
#[cfg(all(windows, target_arch = "x86_64"))]
mod native;
#[cfg(any(test, all(windows, target_arch = "x86_64")))]
mod trial;

#[cfg(test)]
mod tests;

#[cfg(all(windows, target_arch = "x86_64"))]
fn main() -> std::process::ExitCode {
    use std::process::ExitCode;

    let args: Vec<_> = std::env::args_os().skip(1).take(4).collect();
    let (enrollment, evidence, paused) = match args.as_slice() {
        [enrollment, evidence] => (enrollment, evidence, false),
        [flag, enrollment, evidence] if flag == "--paused-trial" => (enrollment, evidence, true),
        _ => {
            eprintln!(
                "usage: arx-vm-qualify [--paused-trial] ENROLLMENT_JSON EMPTY_EVIDENCE_DIRECTORY"
            );
            return ExitCode::FAILURE;
        }
    };
    let result = enrollment
        .to_str()
        .zip(evidence.to_str())
        .ok_or(frame::RunError::Input)
        .and_then(|(enrollment, evidence)| native::run(enrollment, evidence, paused));
    // Only typed, path-free outcomes reach the console.
    match result {
        Ok(native::Outcome::Incomplete) => {
            eprintln!("Incomplete");
            ExitCode::from(2)
        }
        Err(reason) => {
            eprintln!("{}", failure_line(&reason));
            ExitCode::FAILURE
        }
    }
}

/// The console line for a failed run: the typed outcome, path-free, and the
/// next step where it is fixed.
#[cfg(any(test, all(windows, target_arch = "x86_64")))]
fn failure_line(reason: &frame::RunError) -> String {
    match reason {
        frame::RunError::Output => "Output: the diagnostics were not all committed; this claimed \
             evidence directory is consumed, so rerun with a fresh empty directory"
            .to_owned(),
        other => format!("{other:?}"),
    }
}

#[cfg(not(all(windows, target_arch = "x86_64")))]
fn main() -> std::process::ExitCode {
    eprintln!("UnsupportedPlatform");
    std::process::ExitCode::FAILURE
}

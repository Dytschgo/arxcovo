//! Owner-local Windows VM qualification runner (M4 PR 7b1).
//!
//! This binary crate owns the private `TrialAuthority`; the arxd library and
//! the daemon cannot name it, and it never converts into `LaunchAuthority`.
//! The arxd library supplies only an authority-free, read-only preflight
//! and a diagnostic-only evidence sink. In this contract version the
//! executable can write only an incomplete baseline diagnostic.

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

    let args: Vec<_> = std::env::args_os().skip(1).take(3).collect();
    let [enrollment, evidence] = args.as_slice() else {
        eprintln!("usage: arx-vm-qualify ENROLLMENT_JSON EMPTY_EVIDENCE_DIRECTORY");
        return ExitCode::FAILURE;
    };
    let result = enrollment
        .to_str()
        .zip(evidence.to_str())
        .ok_or(frame::RunError::Input)
        .and_then(|(enrollment, evidence)| native::run(enrollment, evidence));
    // Only typed, path-free outcomes reach the console.
    match result {
        Ok(native::Outcome::Incomplete) => {
            eprintln!("Incomplete");
            ExitCode::from(2)
        }
        Err(reason) => {
            eprintln!("{reason:?}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(all(windows, target_arch = "x86_64")))]
fn main() -> std::process::ExitCode {
    eprintln!("UnsupportedPlatform");
    std::process::ExitCode::FAILURE
}

use std::panic::PanicHookInfo;

pub(super) fn report_assertion(phase: &str, source: &str, panic: &PanicHookInfo<'_>) {
    let Some(at) = panic.location().filter(|at| at.file() == source) else {
        println!("\nC13 first phase={phase} category=assertion");
        return;
    };
    let file = source.rsplit(['/', '\\']).next().unwrap_or(source);
    let line = at.line();
    let payload = panic.payload();
    let text = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("");
    // Match only fixed text. Never emit the panic payload or assertion values.
    let summary = match text {
        "C13 process failed before requested readiness" => "process-exited",
        "C13 authenticated readiness elapsed" => "readiness-elapsed",
        "C13 real owner Mint refused" | "C13 survivor Mint refused" => "mint-refused",
        "C13 real join or self-admission refused" => "join-refused",
        "C13 terminal never protected input" => "terminal-unprotected",
        "C13 publication helper failed" => "publication-helper-failed",
        text if text.starts_with("C13 real publication: ") => "publication-refused",
        text if text.starts_with("C13 final possession: ") => "publication-possession-refused",
        text if text.starts_with("assertion `left == right` failed") => "assertion-equality",
        text if text.starts_with("called `Option::unwrap()`")
            || text.starts_with("called `Result::unwrap()`") =>
        {
            "value-unavailable"
        }
        _ => "assertion-failed",
    };
    println!("\nC13 first phase={phase} category=assertion at={file}:{line} message={summary}");
}

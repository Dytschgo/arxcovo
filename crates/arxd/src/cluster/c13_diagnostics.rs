//! Bounded diagnostics for the disposable C13 foreground processes.
use std::{
    io::{self, Write},
    sync::{Mutex, OnceLock},
};

const LIMIT: usize = 24;
type Record = (&'static str, &'static str);

fn fixture(value: &str) -> Option<&'static str> {
    match value {
        "a" => Some("a"),
        "b" => Some("b"),
        "c" => Some("c"),
        "d" => Some("d"),
        _ => None,
    }
}

#[derive(Default)]
struct Records([Option<Record>; LIMIT]);

impl Records {
    fn insert(&mut self, record: Record) -> bool {
        for entry in &mut self.0 {
            match entry {
                Some(prior) if *prior == record => return false,
                None => {
                    *entry = Some(record);
                    return true;
                }
                _ => {}
            }
        }
        false
    }
}

pub(crate) fn record(stage: &'static str, category: &'static str) {
    static FIXTURE: OnceLock<Option<&'static str>> = OnceLock::new();
    static RECORDS: Mutex<Records> = Mutex::new(Records([None; LIMIT]));
    let node = FIXTURE.get_or_init(|| {
        if cfg!(target_os = "linux") {
            std::env::var("ARX_C13_DIAGNOSTICS")
                .ok()
                .as_deref()
                .and_then(fixture)
        } else {
            None
        }
    });
    if let Some(node) = node
        && RECORDS
            .lock()
            .is_ok_and(|mut records| records.insert((stage, category)))
    {
        let _ = writeln!(
            io::stdout().lock(),
            "\nC13 diagnostic node={node} stage={stage} category={category}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_diagnostic_records_are_deduplicated_and_bounded() {
        assert_eq!(fixture("a"), Some("a"));
        assert_eq!(fixture("d"), Some("d"));
        assert_eq!(fixture("a\ncategory=forged"), None);
        let mut records = Records([Some(("stage", "category")); LIMIT]);
        records.0[LIMIT - 1] = None;
        assert!(!records.insert(("stage", "category")));
        assert!(records.insert(("stage", "busy")));
        assert!(!records.insert(("stage", "clock")));
    }
}

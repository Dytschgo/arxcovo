#![cfg(target_os = "linux")]

#[test]
#[ignore = "single hosted C14 grant with the qualified A10 broker"]
fn c14_target_authorized_build_and_lost_reply() {
    assert_eq!(std::env::var("ARX_C14_HOSTED").as_deref(), Ok("1"));
    let root = std::path::PathBuf::from(std::env::var_os("ARX_C14_ROOT").expect("hosted root"));
    assert_eq!(root.parent(), Some(std::path::Path::new("/var/lib")));
    // SAFETY: checking the staged harness identity has no side effects.
    assert_eq!(unsafe { libc::geteuid() }, 0);
    use std::io::{Read, Write};

    // The launcher owns every campaign process and reports success only after
    // verifying the evidence. The wrapper holds no process or cleanup ownership.
    std::io::stderr().write_all(b"C14 wrapper-ready\n").unwrap();
    let mut result = [0];
    std::io::stdin().read_exact(&mut result).unwrap();
    assert_eq!(result, [1], "C14 campaign refused");
}

//! Fixed C13-only publication subprocess. It cannot start a retained node.
use super::super::*;
use std::{
    fs::File,
    io::Read,
    os::fd::{FromRawFd, RawFd},
};

mod c13_failure {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/c13_failure.rs"
    ));
}

const PASTE_FD: RawFd = 3;

#[tokio::test(flavor = "current_thread")]
#[ignore = "one C13 hosted launch through the fixed inherited pipe"]
async fn c13_final_publication_only() {
    assert_eq!(std::env::var("ARX_C13_PUBLISH").as_deref(), Ok("1"));
    std::panic::set_hook(Box::new(|panic| {
        c13_failure::report_assertion("publication", file!(), panic);
    }));
    let config = PathBuf::from(std::env::var_os("ARX_C13_CONFIG").expect("fixed C13 input"));
    // SAFETY: the launcher supplies one owned read end at the fixed descriptor.
    let pipe = unsafe { File::from_raw_fd(PASTE_FD) };
    let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: fstat writes a stat to the provided out pointer on success.
    assert_eq!(unsafe { libc::fstat(PASTE_FD, metadata.as_mut_ptr()) }, 0);
    // SAFETY: fstat succeeded.
    assert_eq!(
        unsafe { metadata.assume_init() }.st_mode & libc::S_IFMT,
        libc::S_IFIFO
    );
    let mut bytes = crate::cluster::pki::custody::voter_transfer::Wipe(Vec::new());
    pipe.take(crate::service::cluster_owner::MAX_PASTE as u64 + 1)
        .read_to_end(&mut bytes.0)
        .expect("publication pipe read");
    let paste = Paste::copy(&bytes.0).expect("publication paste bound");
    drop(bytes);
    let (installation, routes) = crate::service::cluster_runtime::publication_inputs(&config)
        .expect("private publication configuration");
    let join = publish(installation, paste, &routes)
        .await
        .expect("C13 real publication");
    let (published, _, _, _) = join.consume().expect("C13 retained publication");
    let root = published
        .owner
        .read("ca.der", 4096)
        .expect("C13 original root");
    let member = published.custody.member().expect("C13 member custody");
    let now = published
        .time
        .current_time()
        .expect("C13 checked clock")
        .as_secs()
        * 1000;
    crate::cluster::issuance_publication::check_retained(
        &published.owner,
        Some(&published.expected),
        published.custody.installation(),
        &member,
        Some(now),
    )
    .expect("C13 final possession");
    let e = published.expected.origin.position();
    assert_eq!(published.expected.node.get(), 4);
    assert_eq!(
        crate::cluster::pki::ca::fingerprint_of(&root),
        published.expected.ca
    );
    println!(
        "C13 publication node=4 term={} leader={} index={}",
        e.term, e.leader, e.index
    );
}

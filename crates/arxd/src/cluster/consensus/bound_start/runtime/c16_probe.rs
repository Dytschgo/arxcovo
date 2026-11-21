//! One client-only entry for the C16 distinct reconnect. Select by exact name
//! with --ignored --nocapture and pass the owner's paste over anonymous stdin.
use super::*;
use crate::cluster::{
    enrollment::{session::SocketLimits, transfer},
    pki::preproof,
};
use std::{
    io,
    net::{IpAddr, Ipv4Addr, SocketAddrV4},
    os::fd::{AsFd, AsRawFd, OwnedFd},
};
use tokio::{net::unix::pipe::Receiver, time::timeout};

// Fixed C16 fixture route and original A enrollment endpoint.
const ROUTE: LocalRoute = LocalRoute {
    network: IpAddr::V4(Ipv4Addr::new(10, 77, 16, 0)),
    prefix: 24,
};
const ISSUER: SocketAddr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(10, 77, 16, 11), 7444));
const BUDGET: Duration = Duration::from_secs(5);

fn private_pipe(fd: OwnedFd) -> io::Result<Receiver> {
    let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: fd stays open and stat points to writable storage.
    if unsafe { libc::fstatfs(fd.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the successful fstatfs initialized stat.
    if unsafe { stat.assume_init() }.f_type != 0x5049_5045 {
        // Linux PIPEFS_MAGIC
        return Err(io::ErrorKind::InvalidInput.into());
    }
    Receiver::from_owned_fd(fd)
}

async fn read_transfer(pipe: Receiver) -> Option<transfer::Transfer> {
    let paste = Paste::read_probe_pipe(pipe).await.ok()?;
    let transfer = transfer::parse(std::str::from_utf8(paste.bytes()).ok()?, &[ROUTE]).ok()?;
    drop(paste);
    (transfer.endpoint == ISSUER).then_some(transfer)
}

#[tokio::test]
#[ignore = "C16 isolated consumed-code client"]
async fn c16_distinct_reconnect_anchor_only() {
    let outcome = timeout(BUDGET, async {
        let Ok(pipe) = io::stdin()
            .as_fd()
            .try_clone_to_owned()
            .and_then(private_pipe)
        else {
            return "input-refused";
        };
        let Some(transfer) = read_transfer(pipe).await else {
            return "input-refused";
        };
        match preproof::anchor(
            ISSUER,
            &transfer.code,
            &transfer.root,
            Arc::new(FreshTime(HostClock::new(0))),
            SocketLimits::default(),
        )
        .await
        {
            Ok(_) => "anchor-received",
            Err(_) => "anchor-unconfirmed",
        }
    })
    .await
    .unwrap_or("deadline");
    println!("C16_ANCHOR {outcome}");
}

#[tokio::test]
async fn probe_reads_bounded_anonymous_input_and_only_accepts_the_original_endpoint() {
    use crate::cluster::{enrollment::JoinCode, pki::ca::fingerprint_of};
    use tokio::{io::AsyncWriteExt, net::unix::pipe};

    let mut voters = vec![1];
    voters.extend(1u64.to_be_bytes());
    voters.extend([2; 32]);
    let anchor = BoundVoters::decode(&voters).unwrap().anchor([3; 16]);
    let root = b"test-only root";
    let code = JoinCode::generate([3; 16], fingerprint_of(root), anchor, |buffer| {
        buffer.fill(4);
        Ok::<_, ()>(())
    })
    .unwrap();
    for (endpoint, accept) in [
        (ISSUER, true),
        ("10.77.16.11:7445".parse().unwrap(), false),
        ("10.77.17.11:7444".parse().unwrap(), false),
    ] {
        let fixture = transfer::encode(endpoint, root, &code).unwrap();
        let (mut output, input) = pipe::pipe().unwrap();
        let input = private_pipe(input.into_nonblocking_fd().unwrap()).unwrap();
        output
            .write_all(fixture.expose_for_owner_transfer().as_bytes())
            .await
            .unwrap();
        drop(output);
        assert_eq!(read_transfer(input).await.is_some(), accept);
    }

    let (output, input) = pipe::pipe().unwrap();
    assert!(
        timeout(Duration::from_millis(10), read_transfer(input))
            .await
            .is_err()
    );
    drop(output);
    let regular: OwnedFd = std::fs::File::open("/dev/null").unwrap().into();
    assert!(private_pipe(regular).is_err());
}

#[tokio::test]
async fn probe_pipe_requires_eof_inside_its_byte_limit() {
    use crate::service::cluster_owner::MAX_PASTE;
    for size in [0, MAX_PASTE, MAX_PASTE + 1] {
        let bytes = vec![42; size];
        let paste = Paste::read_probe_pipe(bytes.as_slice()).await;
        if size <= MAX_PASTE {
            assert_eq!(paste.unwrap().bytes(), bytes);
        } else {
            assert!(paste.is_err());
        }
    }
}

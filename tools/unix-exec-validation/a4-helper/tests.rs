use super::*;
use std::collections::VecDeque;

enum Step {
    Now(Result<u64, u8>),
    Poll(i32, i16, i32, i64, i16),
    Write(i64),
    Read(i64, u8),
    Close(i32, i64),
}
struct Mock(VecDeque<Step>);
impl Ops for Mock {
    fn now(&mut self) -> Result<u64, u8> {
        match self.0.pop_front().unwrap() {
            Step::Now(v) => v,
            _ => panic!("expected clock"),
        }
    }
    fn poll(&mut self, fd: i32, events: i16, ms: i32) -> (i64, i16) {
        match self.0.pop_front().unwrap() {
            Step::Poll(f, e, t, rc, r) => {
                assert_eq!((fd, events, ms), (f, e, t));
                (rc, r)
            }
            _ => panic!("expected poll"),
        }
    }
    fn write(&mut self, fd: i32, byte: &u8) -> i64 {
        assert_eq!((fd, *byte), (3, b'R'));
        match self.0.pop_front().unwrap() {
            Step::Write(rc) => rc,
            _ => panic!("expected write"),
        }
    }
    fn read(&mut self, fd: i32, byte: &mut u8) -> i64 {
        assert_eq!(fd, 4);
        match self.0.pop_front().unwrap() {
            Step::Read(rc, b) => {
                *byte = b;
                rc
            }
            _ => panic!("expected read"),
        }
    }
    fn close(&mut self, fd: i32) -> i64 {
        match self.0.pop_front().unwrap() {
            Step::Close(f, rc) => {
                assert_eq!(fd, f);
                rc
            }
            _ => panic!("expected close"),
        }
    }
}
fn check(steps: Vec<Step>, expected: u8) {
    let mut m = Mock(steps.into());
    assert_eq!(run(&mut m, 3, 4, LIFE), Err(expected));
    assert!(m.0.is_empty(), "unexpected early return");
}
fn prefix() -> Vec<Step> {
    use Step::*;
    vec![
        Now(Ok(0)),
        Now(Ok(0)),
        Poll(3, 4, 15000, 1, 4),
        Now(Ok(0)),
        Write(1),
        Close(3, 0),
        Now(Ok(0)),
        Poll(4, 1, 15000, 1, 1),
        Now(Ok(0)),
    ]
}

#[test]
fn strict_decimal_and_argv() {
    assert_eq!(
        arguments(&[b"3", b"2147483647", b"9223372036854775807"]),
        Some((3, i32::MAX, i64::MAX as u64))
    );
    for bad in [
        b"".as_slice(),
        b"01",
        b"+3",
        b"-3",
        b" 3",
        b"3 ",
        b"3\n",
        b"3\0",
        b"3x",
        b"99999999999999999999",
        b"18446744073709551615",
    ] {
        assert_eq!(decimal(bad, u64::MAX), None, "{bad:?}");
    }
    for args in [
        vec![b"3".as_slice(), b"4"],
        vec![b"3", b"4", b"1", b"2"],
        vec![b"2", b"4", b"1"],
        vec![b"3", b"3", b"1"],
        vec![b"3", b"4", b"0"],
        vec![b"2147483648", b"4", b"1"],
        vec![b"3", b"4", b"9223372036854775808"],
    ] {
        assert_eq!(arguments(&args), None);
    }
}
#[test]
fn saturating_deadline_rounding() {
    assert_eq!(timeout(0, u64::MAX), Err(EXPIRED));
    assert_eq!(timeout(4, 4), Err(EXPIRED));
    assert_eq!(timeout(1, 0), Ok(1));
    assert_eq!(timeout(1_000_001, 0), Ok(2));
    assert_eq!(timeout(u64::MAX, 0), Ok(i32::MAX));
    for now in 0..=15000 {
        assert_eq!(
            timeout(LIFE, now * 1_000_000),
            if now == 15000 {
                Err(EXPIRED)
            } else {
                Ok(15000 - now as i32)
            }
        );
    }
}
#[test]
fn release_keeps_original_deadline_and_never_succeeds() {
    use Step::*;
    let mut s = prefix();
    s.extend([
        Read(1, b'G'),
        Close(4, 0),
        Now(Ok(9_000_000_000)),
        Poll(-1, 0, 6000, EINTR, 0),
        Now(Ok(14_999_999_999)),
        Poll(-1, 0, 1, 0, 0),
        Now(Ok(LIFE)),
    ]);
    check(s, EXPIRED);
}
#[test]
fn wrong_byte_eof_and_read_errors_fail() {
    for (rc, byte, code) in [(1, b'X', PROTOCOL), (0, 0, IO), (-11, 0, IO), (-5, 0, IO)] {
        let mut s = prefix();
        s.push(Step::Read(rc, byte));
        check(s, code);
    }
}
#[test]
fn only_eintr_retries_with_recomputed_remainder() {
    use Step::*;
    check(
        vec![
            Now(Ok(0)),
            Now(Ok(0)),
            Poll(3, 4, 15000, EINTR, 0),
            Now(Ok(1_000_000_000)),
            Poll(3, 4, 14000, 1, 4),
            Now(Ok(1_000_000_000)),
            Write(EINTR),
            Now(Ok(2_000_000_000)),
            Poll(3, 4, 13000, 1, 4),
            Now(Ok(2_000_000_000)),
            Write(1),
            Close(3, 0),
            Now(Ok(2_000_000_000)),
            Poll(4, 1, 13000, 1, 1),
            Now(Ok(2_000_000_000)),
            Read(EINTR, 0),
            Now(Ok(LIFE)),
        ],
        EXPIRED,
    );
    for rc in [-11, -5, 0, 2] {
        check(
            vec![
                Now(Ok(0)),
                Now(Ok(0)),
                Poll(3, 4, 15000, 1, 4),
                Now(Ok(0)),
                Write(rc),
            ],
            IO,
        );
    }
}
#[test]
fn poll_errors_hup_and_late_readiness_fail() {
    use Step::*;
    for (rc, revents) in [(-11, 0), (-5, 0), (1, 8), (1, 16), (1, 32), (1, 20), (2, 4)] {
        check(
            vec![Now(Ok(0)), Now(Ok(0)), Poll(3, 4, 15000, rc, revents)],
            IO,
        );
    }
    check(
        vec![
            Now(Ok(0)),
            Now(Ok(0)),
            Poll(3, 4, 15000, 1, 4),
            Now(Ok(LIFE)),
        ],
        EXPIRED,
    );
    check(
        vec![
            Now(Ok(0)),
            Now(Ok(0)),
            Poll(3, 4, 15000, 0, 0),
            Now(Ok(LIFE)),
        ],
        EXPIRED,
    );
}
#[test]
fn invalid_clocks_lifetime_and_close_fail_closed() {
    use Step::*;
    check(vec![Now(Err(CLOCK))], CLOCK);
    check(vec![Now(Ok(LIFE))], EXPIRED);
    let mut m = Mock(vec![Now(Ok(0))].into());
    assert_eq!(run(&mut m, 3, 4, LIFE + 1), Err(EXPIRED));
    check(vec![Now(Ok(1)), Now(Ok(0))], CLOCK);
    check(
        vec![
            Now(Ok(0)),
            Now(Ok(0)),
            Poll(3, 4, 15000, 1, 4),
            Now(Ok(0)),
            Write(1),
            Close(3, EINTR),
        ],
        IO,
    );
    let mut s = prefix();
    s.extend([Read(1, b'G'), Close(4, -5)]);
    check(s, IO);
}

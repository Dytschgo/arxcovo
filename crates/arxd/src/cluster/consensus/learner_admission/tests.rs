use super::*;
use crate::cluster::enrollment::NodeId;

#[test]
fn slot_denies_overlap_and_releases_on_drop() {
    let slot = Arc::new(AtomicBool::new(false));
    let owned = Intent::reserve(&slot).unwrap();
    assert!(matches!(Intent::reserve(&slot), Err(Reply::Pending)));
    drop(owned);
    assert!(Intent::reserve(&slot).is_ok());
}

#[test]
fn assumed_peer_cannot_reserve_or_dispatch() {
    let controller = Arc::new(Controller {
        node: Weak::new(),
        closed: Mutex::new(false),
        intent: Arc::default(),
        hooks: Hooks::default(),
    });
    let peer = AuthenticatedPeer::assume(NodeId::new(2).unwrap());
    let result = controller.register(
        peer,
        "192.168.1.2:1234".parse().unwrap(),
        Instant::now() + BUDGET,
        Arc::default(),
    );
    assert!(matches!(result, Err(Reply::Refused)));
    assert!(!controller.intent.load(Ordering::Relaxed));
    assert_eq!(controller.hooks.calls.load(Ordering::Relaxed), 0);
}

#[test]
fn watermarks_compare_exact_identity_at_equality() {
    let p = LogPos {
        index: 12,
        leader: 1,
        term: 2,
    };
    assert!(covers(p, p));
    assert!(!covers(LogPos { leader: 2, ..p }, p));
    assert!(!covers(LogPos { term: 3, ..p }, p));
    assert!(!covers(LogPos { index: 11, ..p }, p));
    assert!(covers(LogPos { index: 13, ..p }, p));
}

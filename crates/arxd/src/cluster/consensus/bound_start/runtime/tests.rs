mod publish_only_probe;

use super::*;

#[test]
fn stop_before_startup_fences_registration_without_dispatching_close() {
    let fence = Admission::default();
    fence.stop();
    let job = Job::anchor();
    fence.register(job);
    {
        let state = job.state();
        assert!(state.process_owned && state.close_requested && !state.close_notified);
    }
    job.request_close();
    assert!(job.state().close_notified);
}

#[test]
fn attach_refusal_keeps_owned_dispatch_and_legacy_close_is_idempotent() {
    let fence = Admission::default();
    let job = Job::anchor();
    fence.register(job);
    job.learner_attach_finished(Err(Refusal::Identity));
    assert_eq!(job.observe().status(), Status::LearnerRefused);
    assert!(!job.state().close_notified);
    let legacy = Job::anchor();
    legacy.learner_attach_finished(Err(Refusal::Identity));
    assert!(legacy.state().close_notified);
}

#[tokio::test]
async fn close_request_notifies_once_after_quiescing() {
    let fence = Admission::default();
    let job = Job::anchor();
    fence.register(job);
    fence.stop();
    assert!(
        tokio::time::timeout(Duration::from_millis(20), job.close.notified())
            .await
            .is_err()
    );
    job.request_close();
    job.close.notified().await;
    job.request_close();
    assert!(
        tokio::time::timeout(Duration::from_millis(20), job.close.notified())
            .await
            .is_err()
    );
}

#[test]
fn membership_coverage_uses_index_and_exact_position_at_equality() {
    let target = LogPos {
        term: 3,
        leader: 1,
        index: 8,
    };
    assert!(covers_log(id(target), id(target)));
    assert!(!covers_log(id(LogPos { term: 4, ..target }), id(target)));
    assert!(!covers_log(
        id(LogPos {
            index: 7,
            term: 4,
            ..target
        }),
        id(target)
    ));
    assert!(covers_log(id(LogPos { index: 9, ..target }), id(target)));
}

#[test]
fn issuer_activation_defers_only_transient_authority_failures() {
    use crate::cluster::consensus::ReadError;
    use enrollment_ingress::ActivationError::{self, Deferred, Refused};
    use membership_observation::MembershipReadError;
    for transient in [ReadError::Busy, ReadError::NotLeader, ReadError::NoQuorum] {
        assert_eq!(ActivationError::from(transient), Deferred);
    }
    assert_eq!(ActivationError::from(ReadError::Unavailable), Refused);
    for transient in [
        MembershipReadError::Busy,
        MembershipReadError::NotLeader,
        MembershipReadError::RetryLater,
    ] {
        assert_eq!(ActivationError::from(transient), Deferred);
    }
    for terminal in [
        MembershipReadError::Unavailable,
        MembershipReadError::PeerRefused,
    ] {
        assert_eq!(ActivationError::from(terminal), Refused);
    }
}

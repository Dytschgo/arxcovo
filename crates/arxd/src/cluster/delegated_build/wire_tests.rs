use super::*;

#[tokio::test]
async fn refused_offer_contains_only_the_selector_profile_and_hash() {
    const PATH: &str = "/fixture/caller-private/workspace";
    const SECRET: &str = "fixture-secret-not-a-credential";
    let command = Descriptor {
        workspace: "target_slot".into(),
        tool: "cargo".into(),
        args: vec!["build".into(), PATH.into(), SECRET.into()],
        timeout_s: 2,
        output_bytes: 128,
    };
    let offer = command.offer().unwrap();
    let refused = Query::Grant(offer);
    let receipt = Query::Spend {
        operation: [1; 32],
        lease: [2; 32],
        descriptor: command.offer().unwrap(),
    };
    for request in [refused, receipt] {
        let mut frame = Vec::new();
        send(&mut frame, &request).await.unwrap();
        let payload = std::str::from_utf8(&frame[4..]).unwrap();
        for denied in [PATH, SECRET, "\"args\"", "\"tool\"", "\"cwd\"", "\"env\""] {
            assert!(!payload.contains(denied));
        }
        assert!(payload.contains("target_slot"));
        assert!(payload.contains("standard_run"));
        assert!(receive::<Query>(&mut frame.as_slice()).await.is_ok());
    }
}

#[tokio::test]
async fn refuses_oversize_trailing_and_unknown_fields() {
    for bytes in [
        (MAX_FRAME as u32 + 1).to_be_bytes().to_vec(),
        [b"\0\0\0\x02".as_slice(), b"{}", b"x"].concat(),
        [b"\0\0\0\x07".as_slice(), br#"{"x":0}"#].concat(),
    ] {
        assert!(receive::<Envelope>(&mut bytes.as_slice()).await.is_err());
    }
}

#[tokio::test]
async fn bounded_records_round_trip() {
    let (mut sender, mut receiver) = tokio::io::duplex(256);
    let sent = Query::Status {
        operation: [42; 32],
    };
    let (sent, received) = tokio::join!(send(&mut sender, &sent), receive::<Query>(&mut receiver));
    sent.unwrap();
    assert!(matches!(received.unwrap(), Query::Status { operation } if operation == [42; 32]));
}

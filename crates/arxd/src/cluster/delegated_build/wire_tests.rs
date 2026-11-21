use super::*;

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

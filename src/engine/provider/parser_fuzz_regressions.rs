use super::sse::{Decoder, Frame};
use super::*;

#[test]
fn sse_comments_stay_separate_from_data_across_every_chunk_boundary() {
    let wire = b": rooxcode waiting\r\n\r\n: ping\n\nevent: message\ndata: {\"choices\":[]}\n\n";
    for size in 1..=wire.len() {
        let mut decoder = Decoder::default();
        let frames: Vec<_> = wire
            .chunks(size)
            .flat_map(|bytes| decoder.push_frames(bytes).unwrap())
            .collect();
        assert_eq!(
            frames,
            vec![
                Frame::Comment("rooxcode waiting".into()),
                Frame::Comment("ping".into()),
                Frame::Data("{\"choices\":[]}".into())
            ]
        );
    }
}

#[test]
fn mixed_sse_delimiters_preserve_wire_order_across_chunks() {
    let wire = b"data: first\r\n\r\ndata: second\n\n";
    for chunk in 1..=wire.len() {
        let mut decoder = Decoder::default();
        let frames: Vec<_> = wire
            .chunks(chunk)
            .flat_map(|bytes| decoder.push(bytes).unwrap())
            .collect();
        assert_eq!(frames, ["first", "second"], "chunk size {chunk}");
    }
}

#[test]
fn rejected_stream_chunk_preserves_the_bounded_buffer() {
    let mut decoder = Decoder::default();
    decoder.push(b"data: pending").unwrap();
    let before = decoder.buffer.clone();
    assert!(decoder.push(&vec![b'x'; 2 * 1024 * 1024]).is_err());
    assert_eq!(decoder.buffer, before);
}

#[test]
fn malformed_anthropic_blocks_return_errors() {
    for block in [
        Value::Null,
        json!(false),
        json!(1),
        json!([]),
        json!("text"),
    ] {
        let mut stream = Stream::default();
        assert!(stream
            .observe(
                &Api::Anthropic,
                &json!({
                    "type":"content_block_start", "index":0, "content_block":block
                })
            )
            .is_err());
    }
    let mut stream = Stream::default();
    stream
        .observe(
            &Api::Anthropic,
            &json!({
                "type":"content_block_start", "index":0,
                "content_block":{"type":"thinking","thinking":"example"}
            }),
        )
        .unwrap();
    assert!(stream
        .observe(
            &Api::Anthropic,
            &json!({
                "type":"content_block_delta", "index":0,
                "delta":{"type":"text_delta","text":"hello"}
            })
        )
        .is_err());
}

use super::*;
use proptest::prelude::*;

fn command_strategy() -> impl Strategy<Value = Command> {
    let variants = (0x0001..=0x010c)
        .filter_map(Command::from_u16)
        .collect::<Vec<_>>();
    assert!(!variants.is_empty());
    proptest::sample::select(variants)
}

fn small_bytes(max: usize) -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 0..max)
}

proptest! {
    #[test]
    fn frame_round_trips_through_encode_and_decode(
        command in command_strategy(),
        flags in any::<u8>(),
        sequence in any::<u32>(),
        payload in small_bytes(256),
    ) {
        let frame = Frame::try_new(command, flags, sequence, payload).unwrap();
        let encoded = frame.encode();
        let (decoded, consumed) = Frame::decode(&encoded).unwrap();
        prop_assert_eq!(consumed, encoded.len());
        prop_assert_eq!(decoded.command, frame.command);
        prop_assert_eq!(decoded.flags.0, frame.flags.0);
        prop_assert_eq!(decoded.sequence, frame.sequence);
        prop_assert_eq!(decoded.payload, frame.payload);
    }

    #[test]
    fn frame_decode_never_panics_on_arbitrary_bytes(data in small_bytes(2048)) {
        let _ = Frame::decode(&data);
    }

    #[test]
    fn frame_decode_consumes_at_most_the_input_length(data in small_bytes(1024)) {
        if let Ok((_, consumed)) = Frame::decode(&data) {
            prop_assert!(consumed <= data.len());
        }
    }

    #[test]
    fn file_chunk_payload_decode_never_panics_on_arbitrary_bytes(data in small_bytes(1024)) {
        let _ = FileChunkPayload::decode(&data);
    }

    #[test]
    fn file_offset_decode_never_panics_on_arbitrary_bytes(data in small_bytes(1024)) {
        let _ = FileOffset::decode(&data);
    }

    #[test]
    fn event_envelope_decode_never_panics_on_arbitrary_bytes(data in small_bytes(1024)) {
        let _ = EventEnvelope::decode(&data);
    }
}

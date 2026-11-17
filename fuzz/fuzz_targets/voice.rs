#![no_main]
use libfuzzer_sys::fuzz_target;
use srs_protocol::voice::Datagram;

fuzz_target!(|bytes: &[u8]| {
    if let Ok(packet) = Datagram::decode(bytes) {
        assert_eq!(packet.encode().expect("decoded packet must encode"), bytes);
    }
});

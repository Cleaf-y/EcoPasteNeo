pub use ecopaste_neo_core::clipboard::native::ClipboardReader;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::ClipboardPayload;
    use crate::settings::Capture;
    use clipboard_rs::{Clipboard, ClipboardContext};

    #[test]
    #[ignore = "touches the real system clipboard; run with --ignored on a desktop session"]
    fn round_trip_text() {
        let _guard = crate::clipboard::test_lock::serial();
        let ctx = ClipboardContext::new().unwrap();
        ctx.set_text("hello ecopaste".to_string()).unwrap();

        let payload = ClipboardReader::new()
            .unwrap()
            .read_with_capture(&Capture::default())
            .unwrap()
            .expect("clipboard should contain text");

        match payload {
            ClipboardPayload::Text(text) => assert_eq!(text.text, "hello ecopaste"),
            other => panic!("expected text payload, got {other:?}"),
        }
    }
}

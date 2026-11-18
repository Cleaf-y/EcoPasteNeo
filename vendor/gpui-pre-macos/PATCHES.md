# Local GPUI macOS fixes

Source: crates.io `gpui-pre-macos 0.3.12` (Zed snapshot
`d2a4fb6ded7c470be25d1541281da3882495b65b`). Apache-2.0 license and
upstream copyright headers are retained. Copied from the separately verified
EcoPasteNeo GPUI prototype on 2026-11-18. Modified upstream files:

- `src/platform.rs`, `MacPlatform::new`: create `GPUIApplication` through
  `sharedApplication` before reading the keyboard layout. Without this order,
  first background launch accepts ASCII but does not start IME composition until
  the application has been activated.
- `src/window.rs`, `build_view_class` and `text_input_window_level`: implement
  `NSTextInputClient.windowLevel` using the actual NSWindow level. Without this,
  system Pinyin candidates are placed behind the nonactivating PopUp panel.

The application uses the supported `runtime_shaders` feature because the local
standalone Metal compiler is unavailable; this is a feature selection, not a
renderer source patch.

Upgrade `gpui-component`, `gpui-pre` and `gpui-pre-platform` together. Compare each
fix against the new published source; remove a local fix only after first-launch
Pinyin composition, candidate ordering, Escape/Enter composition, target-app
focus, and pointer-screen placement pass in the actual EcoPasteNeo.app. Do not
copy the prototype's diagnostic probes or synthetic input into this dependency.

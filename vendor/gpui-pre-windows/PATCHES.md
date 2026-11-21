# Local GPUI Windows fixes

Source: crates.io `gpui-pre-windows 0.3.12` (Zed snapshot
`d2a4fb6ded7c470be25d1541281da3882495b65b`). The source archive SHA-256 is
`7201762420222d5b5ce5bdf0056996199212a3349bfb2457a3a0238c688681f3`, matching
the original workspace lockfile. Apache-2.0 license and upstream copyright
headers are retained.

Modified upstream files:

- `src/directx_renderer.rs`, `shader_resources::build_shader_blob`: compile
  embedded HLSL with `D3DCompile` in debug builds. Upstream uses
  `D3DCompileFromFile` with the compile-time `CARGO_MANIFEST_DIR`, which makes
  a downloaded executable require the CI runner's Cargo source directory.
  Both the main source and alpha correction source are embedded; no runtime
  include handler or external shader files are required.
- `src/shaders.hlsl`: guard the alpha correction include when its contents
  are already embedded. Release builds retain the normal include and existing
  build-time shader compilation.
- `src/directx_renderer.rs`, `embedded_debug_shaders_compile`: test all eight
  shader modules for both vertex and fragment targets, without a GPU or window.
- `src/window.rs`, `WindowsWindow::new`: apply the requested placement with
  `SW_HIDE` when creating a hidden window. Upstream keeps the placement until
  `activate`, but Neo shows its panel through Win32 without calling GPUI's
  activating path. Deferring the bounds leaves the first panel and renderer
  at the OS default size. The saved activation placement is still retained.
  A pure placement test covers hidden, nonactivating and activating options.

Run the regression test on Windows:

```powershell
cargo test -p gpui-pre-windows --lib embedded_debug_shaders_compile --locked
```

The Windows CI step also runs the built test executable while both HLSL source
files are temporarily unavailable, restoring them in `finally`. This detects
accidental reintroduction of runtime source-file reads without requiring a GPU.

Before distributing a debug build, copy only `EcoPasteNeo.exe` to an empty
directory, temporarily make both original HLSL files unavailable, and confirm
`--show --no-capture` creates a working panel with an isolated data directory.
Restore the source files afterward. Merely running from another working
directory does not detect this regression.

Upgrade the GPUI packages together. Compare this patch against the new source
and remove it only after the shader test, hidden placement test, standalone
executable check, and cold first-show checks pass. Test both startup with
`--show` and the first hotkey/tray show after hidden startup, not just reopening
an already rendered panel.
Do not restore runtime access to build-machine paths as a debug convenience.

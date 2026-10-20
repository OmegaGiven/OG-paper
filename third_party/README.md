# Third-party code kept here

- `winit/`: winit 0.30.13 (Apache-2.0, see its LICENSE), with one change:
  on macOS, `set_blur` does nothing and the private CoreGraphics functions
  it called (`CGSMainConnectionID`, `CGSSetWindowBackgroundBlurRadius`) are
  no longer declared, because the Mac App Store rejects binaries that
  reference them (Guideline 2.5.1). Used through `[patch.crates-io]` in the
  workspace `Cargo.toml`. Drop it once winit stops referencing them.

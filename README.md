# dStrafe

`dStrafe` is a Rust port of [`cStrafe UI by CS2Kitchen`](https://github.com/cs2kitchen/cStrafe-UI-minimal), a lightweight counter-strafe training overlay for CS2.

The app listens for movement keys and left mouse clicks, classifies each shot as `Counter-strafe`, `Overlap`, or `Bad`, and renders the result in a draggable always-on-top overlay.

## Build

```bash
cargo build --release
```

## Run

```bash
cargo run --release
```

`dStrafe` is Windows-first. Linux and macOS depend on the platform support and permissions available to `rdev` for global input capture.

To show a debug console and default logs to debug level, set `debug = true` in `dstrafe.toml`.

## Hotkeys

- `F6`: hide or show the overlay
- `Ctrl+F7`: move to display 2 and toggle borderless fullscreen; ignored when only one display is available
- `F8`: exit
- `=`: increase overlay text size
- `-`: decrease overlay text size

## Movement Keys

By default, dStrafe uses WASD. To change this, copy `dstrafe.toml.example` to `dstrafe.toml` in the working directory and edit the single-character ASCII alphanumeric bindings.

```toml
debug = false

[movement]
forward = "W"
backward = "S"
left = "A"
right = "D"
```

# button-async

An async button driver for embedded Rust, built on [`embedded-hal-async`] and [`embassy-time`].

Detects **single-click**, **double-click**, and **long-press** events using a single GPIO pin, with fully configurable timing and active edge.

## Features

- `#![no_std]` — works on any embedded target
- Async-first: integrates naturally with Embassy and other async executors
- Three event types: `SingleClick`, `DoubleClick`, `LongPress`
- Configurable long-press duration and double-click window
- Supports both active-low (`Falling` edge) and active-high (`Rising` edge) buttons

## Usage

```rust
use button_async::{Button, ButtonEvent};

// Assume `pin` implements embedded_hal_async::digital::Wait
let mut button = Button::new(pin);

loop {
    match button.next_event().await {
        ButtonEvent::SingleClick => { /* handle single click */ }
        ButtonEvent::DoubleClick => { /* handle double click */ }
        ButtonEvent::LongPress   => { /* handle long press */ }
    }
}
```

### Custom configuration

```rust
use button_async::{Button, ButtonConfig, ButtonEdge};

let config = ButtonConfig {
    long_press_ms: 800,
    double_click_window_ms: 300,
};
let mut button = Button::with_config(pin, config, ButtonEdge::Rising);
```

## How it works

`next_event()` waits for a press, then races the release against a long-press timer:

- **Release before timeout** → waits for a second press within the double-click window:
  - Second press detected → `DoubleClick`
  - Window expires → `SingleClick`
- **Long-press timer fires first** → waits for release, then returns `LongPress`

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

[`embedded-hal-async`]: https://crates.io/crates/embedded-hal-async
[`embassy-time`]: https://crates.io/crates/embassy-time

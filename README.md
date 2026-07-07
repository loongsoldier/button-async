# button-async

An async button driver for embedded Rust, built on [`embedded-hal-async`] and [`embassy-time`].

Detects **single-click**, **double-click**, and **long-press** events using a single GPIO pin, with fully configurable timing and active edge.

## Features

- `#![no_std]` — works on any embedded target
- Async-first: integrates naturally with Embassy and other async executors
- `Click(u32)` with click count and `LongPress(u32)` with duration in ms
- Configurable long-press duration and multi-click window (using `Duration`)
- Supports both active-low (`Falling` edge) and active-high (`Rising` edge) buttons

## Usage

```rust
use button_async::{Button, ButtonEvent};

// Assume `pin` implements embedded_hal_async::digital::Wait
let mut button = Button::new(pin);

loop {
    match button.next_event().await {
        Ok(ButtonEvent::Click(1)) => { /* handle single click */ }
        Ok(ButtonEvent::Click(2)) => { /* handle double click */ }
        Ok(ButtonEvent::Click(n)) => { /* handle n-click */ }
        Ok(ButtonEvent::LongPress(dur)) => { /* handle long press, dur in ms */ }
        Err(_) => { /* handle pin error */ }
    }
}
```

### Custom configuration

```rust
use button_async::{Button, ButtonConfig, ButtonEdge};
use embassy_time::Duration;

let config = ButtonConfig {
    long_press: Duration::from_millis(800),
    click_window: Duration::from_millis(300),
};
let mut button = Button::with_config(pin, config, ButtonEdge::Rising);
```

## How it works

`next_event()` waits for a press, then races the release against a long-press timer:

- **Release before timeout** → enters a click-counting loop:
  - Count starts at 1, then waits for another press within the click window:
  - Another press detected → count increments, reset window, continue loop
  - Window expires → returns `Click(count)` (1 for single, 2 for double, N for N-click)
- **Long-press timer fires first** → waits for release, returns `LongPress(dur)` where `dur` is the actual hold time in ms

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

[`embedded-hal-async`]: https://crates.io/crates/embedded-hal-async
[`embassy-time`]: https://crates.io/crates/embassy-time

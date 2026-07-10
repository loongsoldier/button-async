# button-async

An async button driver for embedded Rust, built on [`embedded-hal-async`] and [`embassy-time`].

Yields **atomic events** — one per `next_event()` call — so you can react to presses,
releases, multi‑click sequences, and long‑presses as they happen.

## Features

- `#![no_std]` — works on any embedded target
- Async‑first: integrates naturally with Embassy and other async executors
- Atomic events: `Pressed`, `Released { hold_duration, long_press }`, `MultiClick(u32)`
- Multi‑click detection with configurable timeout window
- **Multi‑level** long‑press: `long_press` is the index of the **highest** threshold crossed
- Configurable debounce, active‑low / active‑high

## Usage

```rust
use button_async::{Button, ButtonConfig, ButtonEvent};
use embassy_time::Duration;

// Multi‑level long‑press thresholds, **must be sorted ascending**
static LONG_PRESS_THRESHOLDS: [Duration; 2] = [
    Duration::from_millis(600),   // short hold
    Duration::from_millis(2000),  // long hold
];

let config = ButtonConfig {
    long_press_thresholds: &LONG_PRESS_THRESHOLDS,
    multi_click_timeout: Duration::from_millis(300),
    ..Default::default()
};

// `pin` must implement embedded_hal_async::digital::Wait
let mut button = Button::new(pin, config);

loop {
    match button.next_event().await.unwrap() {
        ButtonEvent::Pressed => {
            // immediate press feedback, e.g. LED on
        }
        ButtonEvent::Released {
            hold_duration,
            long_press,
        } => {
            // LED off
            match long_press {
                Some(0) => { /* short hold */ }
                Some(1) => { /* long hold */ }
                Some(n) => { /* n‑th threshold */ }
                None    => { /* quick release, no long‑press */ }
            }
        }
        ButtonEvent::MultiClick(1) => { /* single click */ }
        ButtonEvent::MultiClick(2) => { /* double click */ }
        ButtonEvent::MultiClick(n) => { /* n‑click */ }
    }
}
```

### Without long‑press detection

Omit (or set to `&[]`) `long_press_thresholds` to disable long‑press:

```rust
let config = ButtonConfig {
    multi_click_timeout: Duration::from_millis(300),
    ..Default::default()
};
// Only Pressed, Released{long_press:None}, and MultiClick are emitted.
```

## Events

Every call to `next_event()` returns exactly one `ButtonEvent`.

| Event | When |
|-------|------|
| `Pressed` | Debounced press confirmed. |
| `Released { hold_duration, long_press }` | Button released. `hold_duration` is actual hold time; `long_press` is `Some(index)` of the **highest** threshold crossed, or `None`. |
| `MultiClick(n)` | Multi‑click window expired after `n` clicks. |

### Example event sequences

```
Single click:
  Pressed → Released{long_press:None} → MultiClick(1)

Double click:
  Pressed → Released{None} → Pressed → Released{None} → MultiClick(2)

Long press 2.5 s (thresholds = [600 ms, 2 s]):
  Pressed
  → (next_event blocks until release)
  → Released{hold:2.5s, long_press:Some(1)}

Double click then long press on third press:
  Pressed → Released{None} → Pressed → Released{None} → Pressed
  → MultiClick(2)
  → (next_event blocks until release)
  → Released{long_press:Some(n)}
```

> **Note:** During a pure long‑press (i.e. the first threshold was crossed
> without a preceding multi‑click), `next_event()` blocks inside the same call
> until release.  Other Embassy tasks continue to run concurrently.

## Configuration

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `debounce` | `Duration` | 20 ms | Debounce time. |
| `long_press_thresholds` | `&[Duration]` | `&[]` | Ascending thresholds. Empty = disabled. |
| `multi_click_timeout` | `Duration` | 300 ms | Max gap between clicks, from last release. |
| `active_low` | `bool` | `true` | `true` = pressed when pin is low. |

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

[`embedded-hal-async`]: https://crates.io/crates/embedded-hal-async
[`embassy-time`]: https://crates.io/crates/embassy-time

#![no_std]

use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Instant, Timer};
use embedded_hal::digital::InputPin;
use embedded_hal_async::digital::Wait;

/// Atomic button event, emitted one at a time by [`Button::next_event`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonEvent {
    /// Debounced press confirmed.
    Pressed,
    /// Button released.
    ///
    /// `hold_duration` is the actual time the button was held.
    /// `long_press` is the index of the **highest** crossed threshold
    /// in [`ButtonConfig::long_press_thresholds`], or `None` if
    /// no threshold was reached.
    Released {
        hold_duration: Duration,
        long_press: Option<usize>,
    },
    /// Multi‑click sequence completed.
    /// `1` = single click, `2` = double click, etc.
    MultiClick(u32),
}

/// Button configuration.
#[derive(Debug, Clone)]
pub struct ButtonConfig<'a> {
    /// Debounce time.
    pub debounce: Duration,
    /// Long‑press thresholds, **must be sorted ascending**.
    /// An empty slice disables long‑press detection entirely.
    pub long_press_thresholds: &'a [Duration],
    /// Multi‑click timeout window, measured from the last release.
    pub multi_click_timeout: Duration,
    /// `true` → low level is pressed, `false` → high level is pressed.
    pub active_low: bool,
}

static EMPTY_THRESHOLDS: [Duration; 0] = [];

impl Default for ButtonConfig<'static> {
    fn default() -> Self {
        Self {
            debounce: Duration::from_millis(20),
            long_press_thresholds: &EMPTY_THRESHOLDS,
            multi_click_timeout: Duration::from_millis(300),
            active_low: true,
        }
    }
}

// ── internal state machine ────────────────────────────────────────

enum State {
    /// Waiting for the first press.
    Idle,
    /// Pressed, waiting for release.
    /// `from_click_count` carries the multi‑click count from a
    /// previous `BetweenClicks`; 0 when coming from `Idle`.
    Pressed {
        pressed_at: Instant,
        from_click_count: u32,
    },
    /// Waiting for release after long‑press has been confirmed
    /// (or after `MultiClick` was already emitted for an interrupted
    /// multi‑click sequence).
    Holding { pressed_at: Instant },
    /// Released, waiting for another press within the multi‑click
    /// window or for the window to expire.
    BetweenClicks { count: u32, last_release: Instant },
}

// ── Button ────────────────────────────────────────────────────────

pub struct Button<'a, P: Wait + InputPin> {
    pin: P,
    config: ButtonConfig<'a>,
    state: State,
}

impl<'a, P: Wait + InputPin> Button<'a, P> {
    pub fn new(pin: P, config: ButtonConfig<'a>) -> Self {
        Self {
            pin,
            config,
            state: State::Idle,
        }
    }

    /// Wait for and return the next atomic button event.
    pub async fn next_event(&mut self) -> Result<ButtonEvent, P::Error> {
        loop {
            match self.state {
                // ── Idle ──────────────────────────────────────
                State::Idle => {
                    self.wait_for_press().await?;
                    let now = Instant::now();
                    self.state = State::Pressed {
                        pressed_at: now,
                        from_click_count: 0,
                    };
                    return Ok(ButtonEvent::Pressed);
                }

                // ── Pressed ───────────────────────────────────
                State::Pressed {
                    pressed_at,
                    from_click_count,
                } => {
                    if !self.config.long_press_thresholds.is_empty() {
                        let elapsed = pressed_at.elapsed();
                        let threshold = self.config.long_press_thresholds[0];

                        // Catch‑up: threshold already passed
                        if elapsed >= threshold {
                            return self.transition_to_holding(pressed_at, from_click_count);
                        }

                        let remaining = threshold - elapsed;
                        match select(self.wait_for_release(), Timer::after(remaining)).await {
                            Either::First(result) => {
                                result?;
                                let hold = pressed_at.elapsed();
                                self.state = State::BetweenClicks {
                                    count: from_click_count + 1,
                                    last_release: Instant::now(),
                                };
                                return Ok(ButtonEvent::Released {
                                    hold_duration: hold,
                                    long_press: None,
                                });
                            }
                            Either::Second(()) => {
                                return self.transition_to_holding(pressed_at, from_click_count);
                            }
                        }
                    } else {
                        // No long‑press thresholds — pure multi‑click
                        self.wait_for_release().await?;
                        let hold = pressed_at.elapsed();
                        self.state = State::BetweenClicks {
                            count: from_click_count + 1,
                            last_release: Instant::now(),
                        };
                        return Ok(ButtonEvent::Released {
                            hold_duration: hold,
                            long_press: None,
                        });
                    }
                }

                // ── Holding ───────────────────────────────────
                State::Holding { pressed_at } => {
                    self.wait_for_release().await?;
                    let hold = pressed_at.elapsed();
                    let long_press = self.highest_long_press_index(hold);
                    self.state = State::Idle;
                    return Ok(ButtonEvent::Released {
                        hold_duration: hold,
                        long_press,
                    });
                }

                // ── BetweenClicks ─────────────────────────────
                State::BetweenClicks {
                    count,
                    last_release,
                } => {
                    let deadline = last_release + self.config.multi_click_timeout;
                    match select(self.wait_for_press(), Timer::at(deadline)).await {
                        Either::First(result) => {
                            result?;
                            let now = Instant::now();
                            self.state = State::Pressed {
                                pressed_at: now,
                                from_click_count: count,
                            };
                            return Ok(ButtonEvent::Pressed);
                        }
                        Either::Second(()) => {
                            self.state = State::Idle;
                            return Ok(ButtonEvent::MultiClick(count));
                        }
                    }
                }
            }
        }
    }

    // ── helpers ───────────────────────────────────────────────

    /// Called when the first long‑press threshold was crossed in
    /// `Pressed`.
    ///
    /// * `from_click_count > 0` → emit `MultiClick` first, then wait
    ///   for release in `Holding` on the next call.
    /// * `from_click_count == 0` → switch to `Holding` silently; the
    ///   outer `loop` will re‑enter and hit the `Holding` arm which
    ///   blocks until release.
    fn transition_to_holding(
        &mut self,
        pressed_at: Instant,
        from_click_count: u32,
    ) -> Result<ButtonEvent, P::Error> {
        if from_click_count > 0 {
            self.state = State::Holding { pressed_at };
            Ok(ButtonEvent::MultiClick(from_click_count))
        } else {
            self.state = State::Holding { pressed_at };
            // Yield nothing — the caller is inside `loop { match … }`,
            // so re‑entering will land on `Holding`.
            panic!(
                "transition_to_holding: pure long press — \
                 this branch must be caught by the outer loop"
            )
        }
    }

    /// Return the index of the highest threshold ≤ `hold`, or `None`.
    fn highest_long_press_index(&self, hold: Duration) -> Option<usize> {
        self.config
            .long_press_thresholds
            .iter()
            .rposition(|&t| t <= hold)
    }

    fn is_pressed(&mut self) -> Result<bool, P::Error> {
        Ok(if self.config.active_low {
            self.pin.is_low()?
        } else {
            self.pin.is_high()?
        })
    }

    async fn wait_for_press(&mut self) -> Result<(), P::Error> {
        let debounce = self.config.debounce;
        loop {
            if self.config.active_low {
                self.pin.wait_for_falling_edge().await?;
            } else {
                self.pin.wait_for_rising_edge().await?;
            }
            Timer::after(debounce).await;
            if self.is_pressed()? {
                return Ok(());
            }
        }
    }

    async fn wait_for_release(&mut self) -> Result<(), P::Error> {
        let debounce = self.config.debounce;
        loop {
            if self.config.active_low {
                self.pin.wait_for_rising_edge().await?;
            } else {
                self.pin.wait_for_falling_edge().await?;
            }
            Timer::after(debounce).await;
            if !self.is_pressed()? {
                return Ok(());
            }
        }
    }
}

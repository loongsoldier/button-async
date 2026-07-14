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
    Released { hold_duration: Duration },
    /// Multi‑click sequence completed.
    /// `1` = single click, `2` = double click, etc.
    MultiClick(u32),
    /// Emitted periodically while the button is held after the long‑press
    /// threshold has been reached.
    ///
    /// `hold_duration` is the total time the button has been held so far.
    /// `count` is a 0‑based counter incremented with each emission.
    ///
    /// Only emitted when [`LongPressConfig::repeat_interval`] is set.
    LongPressRepeat {
        hold_duration: Duration,
        /// 0‑based counter, incremented with each repeat emission.
        count: u32,
    },
}

/// Long‑press configuration.
///
/// When set, the button will detect when it has been held for at least
/// `threshold`.  If `repeat_interval` is also set, periodic
/// [`ButtonEvent::LongPressRepeat`] events are emitted after the
/// threshold is reached until the button is released.
#[derive(Debug, Clone, Copy)]
pub struct LongPressConfig {
    /// Minimum hold time to qualify as a long press.
    pub threshold: Duration,
    /// If set, emit [`ButtonEvent::LongPressRepeat`] at this interval
    /// while the button remains held after `threshold`.
    pub repeat_interval: Option<Duration>,
}

/// Button configuration.
#[derive(Debug, Clone)]
pub struct ButtonConfig {
    /// Debounce time.
    pub debounce: Duration,
    /// Long‑press detection.  `None` disables long‑press entirely.
    pub long_press: Option<LongPressConfig>,
    /// Multi‑click timeout window, measured from the last release.
    pub multi_click_timeout: Duration,
    /// `true` → low level is pressed, `false` → high level is pressed.
    pub active_low: bool,
}

impl Default for ButtonConfig {
    fn default() -> Self {
        Self {
            debounce: Duration::from_millis(20),
            long_press: None,
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
    ///
    /// `repeat_count` is the count to use for the *next*
    /// `LongPressRepeat` emission.
    Holding {
        pressed_at: Instant,
        repeat_count: u32,
    },
    /// Released, waiting for another press within the multi‑click
    /// window or for the window to expire.
    BetweenClicks { count: u32, last_release: Instant },
}

// ── Button ────────────────────────────────────────────────────────

pub struct Button<P: Wait + InputPin> {
    pin: P,
    config: ButtonConfig,
    state: State,
}

impl<P: Wait + InputPin> Button<P> {
    pub fn new(pin: P, config: ButtonConfig) -> Self {
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
                    if let Some(lp) = self.config.long_press {
                        let elapsed = pressed_at.elapsed();
                        let has_repeat = lp.repeat_interval.is_some();

                        // Catch‑up: threshold already passed
                        if elapsed >= lp.threshold {
                            if let Some(event) =
                                self.enter_holding_or_emit(pressed_at, from_click_count, has_repeat)
                            {
                                return Ok(event);
                            }
                            continue;
                        }

                        let remaining = lp.threshold - elapsed;
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
                                });
                            }
                            Either::Second(()) => {
                                if let Some(event) = self.enter_holding_or_emit(
                                    pressed_at,
                                    from_click_count,
                                    has_repeat,
                                ) {
                                    return Ok(event);
                                }
                                continue;
                            }
                        }
                    } else {
                        // No long‑press — pure multi‑click
                        self.wait_for_release().await?;
                        let hold = pressed_at.elapsed();
                        self.state = State::BetweenClicks {
                            count: from_click_count + 1,
                            last_release: Instant::now(),
                        };
                        return Ok(ButtonEvent::Released {
                            hold_duration: hold,
                        });
                    }
                }

                // ── Holding ───────────────────────────────────
                State::Holding {
                    pressed_at,
                    repeat_count,
                } => {
                    if let Some(repeat_interval) =
                        self.config.long_press.and_then(|lp| lp.repeat_interval)
                    {
                        match select(self.wait_for_release(), Timer::after(repeat_interval)).await {
                            Either::First(result) => {
                                result?;
                                let hold = pressed_at.elapsed();
                                self.state = State::Idle;
                                return Ok(ButtonEvent::Released {
                                    hold_duration: hold,
                                });
                            }
                            Either::Second(()) => {
                                let hold = pressed_at.elapsed();
                                let count = repeat_count;
                                self.state = State::Holding {
                                    pressed_at,
                                    repeat_count: repeat_count + 1,
                                };
                                return Ok(ButtonEvent::LongPressRepeat {
                                    hold_duration: hold,
                                    count,
                                });
                            }
                        }
                    } else {
                        self.wait_for_release().await?;
                        let hold = pressed_at.elapsed();
                        self.state = State::Idle;
                        return Ok(ButtonEvent::Released {
                            hold_duration: hold,
                        });
                    }
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

    /// Handle the transition out of `Pressed` when the long‑press
    /// threshold is reached.
    ///
    /// Returns `Some(event)` if an event should be emitted, or `None`
    /// if the caller should `continue` the main loop (state already
    /// updated to `Holding`).
    fn enter_holding_or_emit(
        &mut self,
        pressed_at: Instant,
        from_click_count: u32,
        has_repeat: bool,
    ) -> Option<ButtonEvent> {
        if from_click_count > 0 {
            self.state = State::Holding {
                pressed_at,
                repeat_count: 0,
            };
            return Some(ButtonEvent::MultiClick(from_click_count));
        }
        if has_repeat {
            let hold = pressed_at.elapsed();
            self.state = State::Holding {
                pressed_at,
                repeat_count: 1,
            };
            return Some(ButtonEvent::LongPressRepeat {
                hold_duration: hold,
                count: 0,
            });
        }
        self.state = State::Holding {
            pressed_at,
            repeat_count: 0,
        };
        None
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

        // Fast path: pin may already be at the pressed level
        // (startup, Idle re‑entry while button held, etc.)
        if self.is_pressed()? {
            return Ok(());
        }

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

        // Fast path: pin may already be at the released level
        // (catch‑up: button released during delayed next_event() call,
        //  or select timer won the race before the release edge)
        if !self.is_pressed()? {
            return Ok(());
        }

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

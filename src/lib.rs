#![no_std]

use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Instant, Timer};
use embedded_hal_async::digital::Wait;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonEvent {
    Click(u32),
    LongPress(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonEdge {
    Falling,
    Rising,
}

#[derive(Debug, Clone, Copy)]
pub struct ButtonConfig {
    pub long_press: Duration,
    pub click_window: Duration,
}

impl Default for ButtonConfig {
    fn default() -> Self {
        Self {
            long_press: Duration::from_millis(600),
            click_window: Duration::from_millis(200),
        }
    }
}

pub struct Button<P: Wait> {
    pin: P,
    config: ButtonConfig,
    active_edge: ButtonEdge,
}

impl<P: Wait> Button<P> {
    pub fn new(pin: P) -> Self {
        Self {
            pin,
            config: ButtonConfig::default(),
            active_edge: ButtonEdge::Falling,
        }
    }

    pub fn with_edge(pin: P, edge: ButtonEdge) -> Self {
        Self {
            pin,
            config: ButtonConfig::default(),
            active_edge: edge,
        }
    }

    pub fn with_config(pin: P, config: ButtonConfig, edge: ButtonEdge) -> Self {
        Self {
            pin,
            config,
            active_edge: edge,
        }
    }

    pub async fn next_event(&mut self) -> Result<ButtonEvent, P::Error> {
        let long_press = self.config.long_press;
        let click_window = self.config.click_window;

        self.wait_for_press().await?;
        let pressed_at = Instant::now();

        match select(self.wait_for_release(), Timer::after(long_press)).await {
            Either::First(result) => {
                result?;
                let mut count: u32 = 1;
                loop {
                    match select(self.wait_for_press(), Timer::after(click_window)).await {
                        Either::First(result) => {
                            result?;
                            match select(self.wait_for_release(), Timer::after(long_press)).await {
                                Either::First(result) => {
                                    result?;
                                    count += 1;
                                }
                                Either::Second(()) => {
                                    return Ok(ButtonEvent::Click(count));
                                }
                            }
                        }
                        Either::Second(()) => {
                            return Ok(ButtonEvent::Click(count));
                        }
                    }
                }
            }
            Either::Second(()) => {
                self.wait_for_release().await?;
                let dur = pressed_at.elapsed().as_millis() as u32;
                Ok(ButtonEvent::LongPress(dur))
            }
        }
    }

    async fn wait_for_press(&mut self) -> Result<(), P::Error> {
        match self.active_edge {
            ButtonEdge::Falling => self.pin.wait_for_falling_edge().await,
            ButtonEdge::Rising => self.pin.wait_for_rising_edge().await,
        }
    }

    async fn wait_for_release(&mut self) -> Result<(), P::Error> {
        match self.active_edge {
            ButtonEdge::Falling => self.pin.wait_for_rising_edge().await,
            ButtonEdge::Rising => self.pin.wait_for_falling_edge().await,
        }
    }
}

#![no_std]

use embassy_futures::select::{select, Either};
use embassy_time::{Duration, Timer};
use embedded_hal_async::digital::Wait;

pub enum ButtonEvent {
    SingleClick,
    DoubleClick,
    LongPress,
}

pub enum ButtonEdge {
    Falling,
    Rising,
}

pub struct ButtonConfig {
    pub long_press_ms: u64,
    pub double_click_window_ms: u64,
}

impl Default for ButtonConfig {
    fn default() -> Self {
        Self {
            long_press_ms: 600,
            double_click_window_ms: 200,
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

    pub async fn next_event(&mut self) -> ButtonEvent {
        let long_ms = self.config.long_press_ms;
        let double_ms = self.config.double_click_window_ms;

        self.wait_for_press().await;

        match select(
            self.wait_for_release(),
            Timer::after(Duration::from_millis(long_ms)),
        )
        .await
        {
            Either::First(_) => {
                match select(
                    self.wait_for_press(),
                    Timer::after(Duration::from_millis(double_ms)),
                )
                .await
                {
                    Either::First(_) => {
                        self.wait_for_release().await;
                        ButtonEvent::DoubleClick
                    }
                    Either::Second(_) => ButtonEvent::SingleClick,
                }
            }
            Either::Second(_) => {
                self.wait_for_release().await;
                ButtonEvent::LongPress
            }
        }
    }

    async fn wait_for_press(&mut self) {
        match self.active_edge {
            ButtonEdge::Falling => self.pin.wait_for_falling_edge().await.ok(),
            ButtonEdge::Rising => self.pin.wait_for_rising_edge().await.ok(),
        };
    }

    async fn wait_for_release(&mut self) {
        match self.active_edge {
            ButtonEdge::Falling => self.pin.wait_for_rising_edge().await.ok(),
            ButtonEdge::Rising => self.pin.wait_for_falling_edge().await.ok(),
        };
    }
}

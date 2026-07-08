#![no_std]

use derive_setters::Setters;
use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Instant, Timer};
use embedded_hal::digital::{ErrorType, InputPin};
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

#[derive(Debug, Clone, Copy, Setters)]
#[setters(no_std)]
pub struct ButtonConfig {
    #[setters(into)]
    pub long_press: Duration,
    #[setters(into)]
    pub click_window: Duration,
    #[setters(into)]
    pub debounce: Duration,
    pub active_edge: ButtonEdge,
}

impl Default for ButtonConfig {
    fn default() -> Self {
        Self {
            long_press: Duration::from_millis(600),
            click_window: Duration::from_millis(200),
            debounce: Duration::from_millis(20),
            active_edge: ButtonEdge::Falling,
        }
    }
}

pub struct Button<P: Wait + InputPin> {
    pin: P,
    config: ButtonConfig,
}

impl<P: Wait + InputPin> Button<P> {
    pub fn new(pin: P, config: ButtonConfig) -> Self {
        Self { pin, config }
    }

    pub async fn next_event(&mut self) -> Result<ButtonEvent, <P as ErrorType>::Error> {
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

    fn is_pressed(&mut self) -> Result<bool, <P as ErrorType>::Error> {
        Ok(match self.config.active_edge {
            ButtonEdge::Falling => self.pin.is_low()?,
            ButtonEdge::Rising => self.pin.is_high()?,
        })
    }

    async fn wait_for_press(&mut self) -> Result<(), <P as ErrorType>::Error> {
        let debounce = self.config.debounce;
        loop {
            match self.config.active_edge {
                ButtonEdge::Falling => self.pin.wait_for_falling_edge().await?,
                ButtonEdge::Rising => self.pin.wait_for_rising_edge().await?,
            }
            Timer::after(debounce).await;
            if self.is_pressed()? {
                return Ok(());
            }
        }
    }

    async fn wait_for_release(&mut self) -> Result<(), <P as ErrorType>::Error> {
        let debounce = self.config.debounce;
        loop {
            match self.config.active_edge {
                ButtonEdge::Falling => self.pin.wait_for_rising_edge().await?,
                ButtonEdge::Rising => self.pin.wait_for_falling_edge().await?,
            }
            Timer::after(debounce).await;
            if !self.is_pressed()? {
                return Ok(());
            }
        }
    }
}

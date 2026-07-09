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

/// Hook that runs after a debounced button press is detected.
///
/// The [`pressed`](OnPress::pressed) method is called right after
/// [`wait_for_press`] succeeds, before release / long‑press detection begins.
/// Use it for press feedback such as lighting an LED or starting haptic feedback.
#[allow(async_fn_in_trait)]
pub trait OnPress {
    /// Called when a debounced press is confirmed.
    async fn pressed(&mut self);
}

/// Default no‑op press handler.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoOpOnPress;

impl OnPress for NoOpOnPress {
    async fn pressed(&mut self) {}
}

#[derive(Debug, Clone, Setters)]
#[setters(no_std)]
pub struct ButtonConfig<H: OnPress = NoOpOnPress> {
    #[setters(into)]
    pub long_press: Duration,
    #[setters(into)]
    pub click_window: Duration,
    #[setters(into)]
    pub debounce: Duration,
    pub active_edge: ButtonEdge,
    /// Handler invoked when a debounced press is detected.
    pub on_press: H,
}

impl Default for ButtonConfig<NoOpOnPress> {
    fn default() -> Self {
        Self {
            long_press: Duration::from_millis(600),
            click_window: Duration::from_millis(200),
            debounce: Duration::from_millis(20),
            active_edge: ButtonEdge::Falling,
            on_press: NoOpOnPress,
        }
    }
}

/// Keep `Copy` for the common no‑handler case.
impl Copy for ButtonConfig<NoOpOnPress> {}

impl<H: OnPress> ButtonConfig<H> {
    /// Replace the press handler, returning a config with a new handler type.
    ///
    /// This allows chaining with other setters regardless of order:
    ///
    /// ```ignore
    /// let config = ButtonConfig::default()
    ///     .long_press(Duration::from_millis(1000))
    ///     .with_on_press(my_handler)
    ///     .active_edge(ButtonEdge::Rising);
    /// ```
    pub fn with_on_press<H2: OnPress>(self, handler: H2) -> ButtonConfig<H2> {
        ButtonConfig {
            long_press: self.long_press,
            click_window: self.click_window,
            debounce: self.debounce,
            active_edge: self.active_edge,
            on_press: handler,
        }
    }
}

pub struct Button<P: Wait + InputPin, H: OnPress = NoOpOnPress> {
    pin: P,
    config: ButtonConfig<H>,
}

impl<P: Wait + InputPin, H: OnPress> Button<P, H> {
    pub fn new(pin: P, config: ButtonConfig<H>) -> Self {
        Self { pin, config }
    }

    pub async fn next_event(&mut self) -> Result<ButtonEvent, <P as ErrorType>::Error> {
        let long_press = self.config.long_press;
        let click_window = self.config.click_window;

        self.wait_for_press().await?;
        let pressed_at = Instant::now();
        self.config.on_press.pressed().await;

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

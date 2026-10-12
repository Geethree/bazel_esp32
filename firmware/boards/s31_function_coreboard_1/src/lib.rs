#![no_std]

use board_api::{ResetButton, StatusIndicator};
use esp_hal::{
    Async, Blocking,
    gpio::{Input, InputConfig, Level, Pull},
    rmt::{Channel, PulseCode, Rmt, Tx, TxChannelConfig, TxChannelCreator},
    time::Rate,
    uart::Uart,
};

mod provisioning;
pub use provisioning::{AP_PREFIX, WIFI_CFG_OFFSET};

pub const NAME: &str = "ESP32-S31 Function CoreBoard-1";
pub type Console = Uart<'static, Async>;

#[macro_export]
macro_rules! take_console {
    ($peripherals:ident) => {
        esp_hal::uart::Uart::new($peripherals.UART0, esp_hal::uart::Config::default())
            .expect("S31 console UART")
            .with_tx($peripherals.GPIO58)
            .with_rx($peripherals.GPIO59)
            .into_async()
    };
}

#[macro_export]
macro_rules! take_button {
    ($peripherals:ident) => {
        $crate::boot_button($peripherals.GPIO61)
    };
}

#[macro_export]
macro_rules! take_led {
    ($peripherals:ident) => {
        $crate::StatusLed::new($peripherals.RMT, $peripherals.GPIO60)
    };
}

pub fn boot_button(pin: impl esp_hal::gpio::InputPin + 'static) -> BootButton {
    BootButton(Input::new(pin, InputConfig::default().with_pull(Pull::Up)))
}

pub struct BootButton(Input<'static>);

impl ResetButton for BootButton {
    fn is_released(&self) -> bool {
        self.0.is_high()
    }
}

pub struct StatusLed {
    channel: Option<Channel<'static, Blocking, Tx>>,
    on: bool,
    error_reported: bool,
}

impl StatusLed {
    pub fn new(
        rmt: esp_hal::peripherals::RMT<'static>,
        pin: impl esp_hal::gpio::OutputPin + 'static,
    ) -> Self {
        let frequency = Rate::from_mhz(80);
        let rmt = Rmt::new(rmt, frequency).expect("S31 RMT");
        let channel = rmt
            .channel0
            .configure_tx(
                &TxChannelConfig::default()
                    .with_clk_divider(1)
                    .with_idle_output_level(Level::Low)
                    .with_idle_output(true)
                    .with_memsize(2),
            )
            .expect("S31 RGB LED")
            .with_pin(pin);
        Self {
            channel: Some(channel),
            on: false,
            error_reported: false,
        }
    }
}

impl StatusIndicator for StatusLed {
    fn set(&mut self, on: bool) {
        self.on = on;
        // One WS2812 pixel, GRB order. RMT runs at 80 MHz (12.5 ns/tick).
        let grb = if on { 64_u32 } else { 0 };
        let mut pulses = [PulseCode::end_marker(); 26];
        for (index, pulse) in pulses[..24].iter_mut().enumerate() {
            *pulse = if grb & (1 << (23 - index)) != 0 {
                PulseCode::new(Level::High, 64, Level::Low, 48)
            } else {
                PulseCode::new(Level::High, 28, Level::Low, 56)
            };
        }
        pulses[24] = PulseCode::new(Level::Low, 6400, Level::Low, 1);

        if let Some(channel) = self.channel.take() {
            let (channel, error) = match channel.transmit(&pulses) {
                Ok(transaction) => match transaction.wait() {
                    Ok(channel) => (channel, None),
                    Err((error, channel)) => (channel, Some(error)),
                },
                Err((error, channel)) => (channel, Some(error)),
            };
            self.channel = Some(channel);
            if let Some(error) = error {
                if !self.error_reported {
                    esp_println::println!("RGB LED RMT transmit failed: {error:?}");
                    self.error_reported = true;
                }
            } else {
                self.error_reported = false;
            }
        }
    }

    fn toggle(&mut self) {
        self.set(!self.on);
    }
}

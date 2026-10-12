#![no_std]

use board_api::{ResetButton, StatusIndicator};
use esp_hal::{
    Async,
    gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull},
    usb::usb_serial_jtag::UsbSerialJtag,
};

mod provisioning;
pub use provisioning::{AP_PREFIX, WIFI_CFG_OFFSET};

pub const NAME: &str = "XIAO ESP32-C6";
pub type Console = UsbSerialJtag<'static, Async>;

#[macro_export]
macro_rules! take_console {
    ($peripherals:ident) => {
        esp_hal::usb::usb_serial_jtag::UsbSerialJtag::new($peripherals.USB_DEVICE).into_async()
    };
}

#[macro_export]
macro_rules! take_button {
    ($peripherals:ident) => {
        $crate::boot_button($peripherals.GPIO9)
    };
}

#[macro_export]
macro_rules! take_led {
    ($peripherals:ident) => {
        $crate::StatusLed::new($peripherals.GPIO15)
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
    pin: Output<'static>,
    on: bool,
}

impl StatusLed {
    pub fn new(pin: impl esp_hal::gpio::OutputPin + 'static) -> Self {
        Self {
            // XIAO ESP32-C6 GPIO15 user LED is active low.
            pin: Output::new(pin, Level::High, OutputConfig::default()),
            on: false,
        }
    }
}

impl StatusIndicator for StatusLed {
    fn set(&mut self, on: bool) {
        self.on = on;
        self.pin
            .set_level(if on { Level::Low } else { Level::High });
    }

    fn toggle(&mut self) {
        self.set(!self.on);
    }
}

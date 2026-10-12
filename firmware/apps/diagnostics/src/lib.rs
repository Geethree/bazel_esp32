#![no_std]

use board_api::{ResetButton, StatusIndicator};
use esp_hal::delay::Delay;

pub fn run() -> ! {
    esp_println::println!("{} diagnostics boot", board::NAME);
    let peripherals = esp_hal::init(esp_hal::Config::default());
    esp_println::println!("HAL ready");
    let button = board::take_button!(peripherals);
    esp_println::println!("BOOT input ready");
    let mut led = board::take_led!(peripherals);
    esp_println::println!("{} diagnostics ready", board::NAME);
    let delay = Delay::new();
    let mut released = button.is_released();
    loop {
        led.toggle();
        let next = button.is_released();
        if next != released {
            released = next;
            esp_println::println!("BOOT {}", if released { "released" } else { "pressed" });
        }
        // This bare-metal diagnostic deliberately polls every 500 ms. It keeps
        // the CPU awake; provisioning uses the RTOS scheduler instead.
        delay.delay_millis(500);
    }
}

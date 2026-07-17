#![no_main]
#![no_std]
#![deny(clippy::mem_forget)]

use esp_hal::{
    clock::CpuClock,
    main,
    time::{Duration, Instant},
    efuse::{self, InterfaceMacAddress},
};

#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    esp_println::println!("panic: {info}");
    loop {}
}

esp_bootloader_esp_idf::esp_app_desc!();

#[main]
fn main() -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let _peripherals = esp_hal::init(config);

    let chip_rev = efuse::chip_revision();
    let mac = efuse::interface_mac_address(InterfaceMacAddress::Station);

    loop {
        esp_println::println!(
            "Hello from ESP32-C6 (Silicon Rev {}.{}), MAC: {:02x?}, built hermetically with Bazel!",
            chip_rev.major,
            chip_rev.minor,
            mac.as_bytes()
        );

        let delay_start = Instant::now();
        while delay_start.elapsed() < Duration::from_secs(1) {}
    }
}

#![no_std]
#![no_main]
#![deny(clippy::mem_forget)]

esp_bootloader_esp_idf::esp_app_desc!();

#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    esp_println::println!("panic: {info}");
    loop {
        core::hint::spin_loop();
    }
}

#[esp_rtos::main]
async fn main(spawner: embassy_executor::Spawner) -> ! {
    firmware_provisioning_app::run(spawner).await
}

use board_api::{ResetButton, StatusIndicator};
use embassy_time::{Duration, Timer};

pub async fn reset_requested<B: ResetButton, L: StatusIndicator>(
    button: &B,
    led: &mut L,
    window_seconds: u8,
    hold_seconds: u8,
) -> bool {
    const SAMPLE_MS: u64 = 50;
    let samples = u64::from(window_seconds) * 1_000 / SAMPLE_MS;
    let hold_samples = u64::from(hold_seconds) * 1_000 / SAMPLE_MS;
    // Require a release after boot before accepting a reset gesture.
    let mut armed = button.is_released();
    let mut held = 0_u64;
    for sample in 0..samples {
        Timer::after(Duration::from_millis(SAMPLE_MS)).await;
        if sample % (250 / SAMPLE_MS) == 0 {
            led.toggle();
        }
        if !armed {
            armed = button.is_released();
            continue;
        }
        if !button.is_released() {
            held += 1;
            if held >= hold_samples {
                return true;
            }
        } else {
            held = 0;
        }
    }
    false
}

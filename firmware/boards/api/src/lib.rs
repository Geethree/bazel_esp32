#![no_std]

pub trait StatusIndicator {
    fn set(&mut self, on: bool);
    fn toggle(&mut self);
}

pub trait ResetButton {
    fn is_released(&self) -> bool;
}

//! Provisioning-specific payloads carried by the generic HIL protocol.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Control {
    Reset = 1,
    ClearStorage = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnknownControl;

impl Control {
    pub const fn encode(self) -> [u8; 1] {
        [self as u8]
    }

    pub fn decode(payload: &[u8]) -> Result<Self, UnknownControl> {
        match payload {
            [1] => Ok(Self::Reset),
            [2] => Ok(Self::ClearStorage),
            _ => Err(UnknownControl),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_round_trip() {
        for control in [Control::Reset, Control::ClearStorage] {
            assert_eq!(Control::decode(&control.encode()), Ok(control));
        }
    }

    #[test]
    fn unknown_controls_are_rejected() {
        assert_eq!(Control::decode(&[0]), Err(UnknownControl));
        assert_eq!(Control::decode(&[1, 2]), Err(UnknownControl));
    }
}

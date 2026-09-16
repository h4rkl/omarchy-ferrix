//! Stable, target-neutral hardware contracts.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PowerState {
    pub battery_percent: u8,
    pub charging: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PowerEvent {
    Changed(PowerState),
}

pub trait Power {
    fn state(&self) -> PowerState;
}

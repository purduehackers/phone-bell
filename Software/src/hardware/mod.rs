pub mod audio;
#[cfg(not(feature = "real"))]
pub mod emulated;
#[cfg(feature = "real")]
pub mod physical;

pub trait PhoneHardware {
    fn create() -> Self;

    fn update(&mut self);

    fn ring(&mut self, enabled: bool);

    fn enable_dialing(&mut self, enabled: bool);

    fn dialed_number(&mut self) -> &mut String;

    fn get_hook_state(&self) -> bool;
}

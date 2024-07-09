use std::time::{Duration, Instant};

#[cfg(not(target_family = "windows"))]
use rppal::gpio::{Gpio, InputPin, Level, OutputPin};

#[cfg(not(target_family = "windows"))]
use crate::config::{BELL_SOLENOID_PIN, DIAL_LATCH_PIN, DIAL_PULSE_PIN, HOOK_SWITCH_PIN};

pub struct Hardware {
    #[cfg(not(target_family = "windows"))]
    gpio: Gpio,

    #[cfg(not(target_family = "windows"))]
    hook_switch: InputPin,

    #[cfg(not(target_family = "windows"))]
    dial_latch: InputPin,
    #[cfg(not(target_family = "windows"))]
    dial_pulse: InputPin,

    #[cfg(not(target_family = "windows"))]
    bell_solenoid: OutputPin,

    last_update_instant: Instant,

    ringing_bell: bool,
    bell_ring_timer: Duration,
    current_bell_signal: bool,

    last_dial_pulse_state: bool,
    dialing_enabled: bool,
    pub dialed_number: String,
    dial_pulses: i32,
}

#[cfg(not(target_family = "windows"))]
pub fn create() -> Hardware {
    let gpio = Gpio::new()?;

    let hook_switch = gpio.get(HOOK_SWITCH_PIN)?.into_input();

    let dial_latch = gpio.get(DIAL_LATCH_PIN)?.into_input();
    let dial_pulse = gpio.get(DIAL_PULSE_PIN)?.into_input();

    let bell_solenoid = gpio.get(BELL_SOLENOID_PIN)?.into_output();

    Hardware {
        // TODO: Add audio infrastructure
        gpio,

        hook_switch,

        dial_latch,
        dial_pulse,

        bell_solenoid,

        last_update_instant: Instant::now(),

        ringing_bell: false,
        bell_ring_timer: Duration::ZERO,
        current_bell_signal: false,

        last_dial_pulse_state: false,
        dialing_enabled: false,
        dialed_number: String::new(),
        dial_pulses: 0,
    }
}

#[cfg(target_family = "windows")]
pub fn create() -> Hardware {
    Hardware {
        // TODO: Add audio infrastructure
        last_update_instant: Instant::now(),

        ringing_bell: false,
        bell_ring_timer: Duration::ZERO,
        current_bell_signal: false,

        last_dial_pulse_state: false,
        dialing_enabled: false,
        dialed_number: String::new(),
        dial_pulses: 0,
    }
}

impl Hardware {
    pub fn update(&mut self) {
        let now = Instant::now();

        self.bell_ring_timer += self.last_update_instant.duration_since(now);

        self.last_update_instant = now;

        if self.bell_ring_timer >= Duration::from_secs_f64(0.05) {
            self.bell_ring_timer = Duration::ZERO;

            self.current_bell_signal = !self.current_bell_signal & self.ringing_bell;

            #[cfg(not(target_family = "windows"))]
            self.bell_solenoid.write(if self.current_bell_signal {
                Level::High
            } else {
                Level::Low
            });
        }

        #[cfg(not(target_family = "windows"))]
        let dial_latch_state = self.dial_latch.is_high();
        #[cfg(not(target_family = "windows"))]
        let dial_pulse_state = self.dial_pulse.is_high();

        #[cfg(target_family = "windows")]
        let dial_latch_state = false;
        #[cfg(target_family = "windows")]
        let dial_pulse_state = false;

        if dial_latch_state {
            if self.last_dial_pulse_state != dial_pulse_state && dial_pulse_state {
                self.dial_pulses += 1;
            }
        } else if self.dial_pulses > 0 {
            if self.dial_pulses >= 10 {
                self.dialed_number += "0";
            } else {
                self.dialed_number += &self.dial_pulses.to_string();
            }

            self.dial_pulses = 0;
        }

        self.last_dial_pulse_state = dial_pulse_state;
    }

    pub fn ring(&mut self, enabled: bool) {
        self.ringing_bell = enabled;
    }

    pub fn get_hook_state(&self) -> bool {
        #[cfg(not(target_family = "windows"))]
        return self.hook_switch.is_high();

        #[cfg(target_family = "windows")]
        return true;
    }

    pub fn enable_dialing(&mut self, enabled: bool) {
        self.dialing_enabled = enabled;
    }
}

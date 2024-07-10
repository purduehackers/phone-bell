use std::time::{Duration, Instant};

use debouncr::{debounce_4, Debouncer, Repeat4};

#[cfg(not(target_family = "windows"))]
use rppal::gpio::{Gpio, InputPin, Level, OutputPin};

#[cfg(not(target_family = "windows"))]
use crate::config::{BELL_SOLENOID_PIN, DIAL_LATCH_PIN, DIAL_PULSE_PIN, HOOK_SWITCH_PIN};

pub struct Hardware {
    #[cfg(not(target_family = "windows"))]
    gpio: Gpio,

    #[cfg(not(target_family = "windows"))]
    hook_switch: InputPin,
    hook_switch_debounce: Debouncer<u8, Repeat4>,

    #[cfg(not(target_family = "windows"))]
    dial_latch: InputPin,
    dial_latch_debounce: Debouncer<u8, Repeat4>,
    #[cfg(not(target_family = "windows"))]
    dial_pulse: InputPin,
    dial_pulse_debounce: Debouncer<u8, Repeat4>,

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
    let Ok(gpio) = Gpio::new() else {
        panic!("Failed to initialize GPIO")
    };

    let Ok(hook_switch) = gpio.get(HOOK_SWITCH_PIN) else {
        panic!("Failed to get pin")
    };

    let Ok(dial_latch) = gpio.get(DIAL_LATCH_PIN) else {
        panic!("Failed to get pin")
    };

    let Ok(dial_pulse) = gpio.get(DIAL_PULSE_PIN) else {
        panic!("Failed to get pin")
    };

    let Ok(bell_solenoid) = gpio.get(BELL_SOLENOID_PIN) else {
        panic!("Failed to get pin")
    };

    Hardware {
        // TODO: Add audio infrastructure
        gpio,

        hook_switch: hook_switch.into_input(),
        hook_switch_debounce: debounce_4(false),

        dial_latch: dial_latch.into_input(),
        dial_latch_debounce: debounce_4(false),
        dial_pulse: dial_pulse.into_input(),
        dial_pulse_debounce: debounce_4(false),

        bell_solenoid: bell_solenoid.into_output(),

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

        hook_switch_debounce: debounce_4(false),
        
        dial_latch_debounce: debounce_4(false),
        dial_pulse_debounce: debounce_4(false),

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
        #[cfg(not(target_family = "windows"))]
        self.hook_switch_debounce.update(self.hook_switch.is_high());

        #[cfg(not(target_family = "windows"))]
        self.dial_latch_debounce.update(self.dial_latch.is_high());
        #[cfg(not(target_family = "windows"))]
        self.dial_pulse_debounce.update(self.dial_pulse.is_low());

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

        let dial_latch_state = self.dial_latch_debounce.is_high();
        let dial_pulse_state = self.dial_pulse_debounce.is_high();

        if dial_latch_state {
            if self.last_dial_pulse_state != dial_pulse_state && dial_pulse_state {
                self.dial_pulses += 1;
            }
        } else if self.dial_pulses > 0 {
            if self.dial_pulses >= 10 && self.dialing_enabled {
                self.dialed_number += "0";
            } else if self.dialing_enabled {
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
        self.hook_switch_debounce.is_high()
    }

    pub fn enable_dialing(&mut self, enabled: bool) {
        self.dialing_enabled = enabled;
    }
}

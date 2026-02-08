use std::time::{Duration, Instant};

use debouncr::{debounce_4, Debouncer, Repeat4};

use crate::hardware::PhoneHardware;

use rppal::gpio::{Gpio, InputPin, Level, OutputPin};

use crate::config::{
    BELL_SOLENOID_FORWARD_PIN, BELL_SOLENOID_REVERSE_PIN, DIAL_LATCH_PIN, DIAL_PULSE_PIN,
    HOOK_SWITCH_PIN,
};

pub struct Hardware {
    last_update_instant: Instant,

    gpio_read_timer: Duration,

    gpio: Gpio,

    hook_switch: InputPin,
    hook_switch_debounce: Debouncer<u8, Repeat4>,

    dial_latch: InputPin,
    dial_latch_debounce: Debouncer<u8, Repeat4>,
    dial_pulse: InputPin,
    dial_pulse_debounce: Debouncer<u8, Repeat4>,

    bell_solenoid_forward: OutputPin,
    bell_solenoid_reverse: OutputPin,

    ringing_bell: bool,
    bell_ring_timer: Duration,
    current_bell_signal: bool,

    last_dial_pulse_state: bool,
    dialing_enabled: bool,
    dialed_number: String,
    dial_pulses: i32,
}

impl PhoneHardware for Hardware {
    fn create() -> Self {
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

        let Ok(bell_solenoid_forward) = gpio.get(BELL_SOLENOID_FORWARD_PIN) else {
            panic!("Failed to get pin")
        };

        let Ok(bell_solenoid_reverse) = gpio.get(BELL_SOLENOID_REVERSE_PIN) else {
            panic!("Failed to get pin")
        };

        Hardware {
            last_update_instant: Instant::now(),

            gpio_read_timer: Duration::ZERO,

            gpio,

            hook_switch: hook_switch.into_input(),
            hook_switch_debounce: debounce_4(false),

            dial_latch: dial_latch.into_input(),
            dial_latch_debounce: debounce_4(false),
            dial_pulse: dial_pulse.into_input(),
            dial_pulse_debounce: debounce_4(false),

            bell_solenoid_forward: bell_solenoid_forward.into_output(),
            bell_solenoid_reverse: bell_solenoid_reverse.into_output(),

            ringing_bell: false,
            bell_ring_timer: Duration::ZERO,
            current_bell_signal: false,

            last_dial_pulse_state: false,
            dialing_enabled: false,
            dialed_number: String::new(),
            dial_pulses: 0,
        }
    }

    fn update(&mut self) {
        let now = Instant::now();

        let time_delta = now.duration_since(self.last_update_instant);

        self.gpio_read_timer += time_delta;
        self.bell_ring_timer += time_delta;

        self.last_update_instant = now;

        if self.gpio_read_timer >= Duration::from_millis(1) {
            // Holy mother of god, 1.4GHz is fast, delay!
            self.gpio_read_timer = Duration::ZERO;

            self.hook_switch_debounce.update(self.hook_switch.is_high());

            self.dial_latch_debounce.update(self.dial_latch.is_high());
            self.dial_pulse_debounce.update(self.dial_pulse.is_low());
        }

        if self.bell_ring_timer >= Duration::from_millis(50) {
            self.bell_ring_timer = Duration::ZERO;

            self.current_bell_signal = !self.current_bell_signal & self.ringing_bell;

            if self.current_bell_signal {
                self.bell_solenoid_forward.set_high();
                self.bell_solenoid_reverse.set_low();
            } else {
                self.bell_solenoid_forward.set_low();
                self.bell_solenoid_reverse.set_high();
            }
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

    fn ring(&mut self, enabled: bool) {
        self.ringing_bell = enabled;
    }

    fn enable_dialing(&mut self, enabled: bool) {
        self.dialing_enabled = enabled;
    }

    fn dialed_number(&mut self) -> &mut String {
        &mut self.dialed_number
    }

    fn get_hook_state(&self) -> bool {
        self.hook_switch_debounce.is_high()
    }
}

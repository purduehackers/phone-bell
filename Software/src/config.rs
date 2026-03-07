pub const HOOK_SWITCH_PIN: u8 = 17;

pub const DIAL_LATCH_PIN: u8 = 22;
pub const DIAL_PULSE_PIN: u8 = 27;

pub const BELL_SOLENOID_FORWARD_PIN: u8 = 24;
pub const BELL_SOLENOID_REVERSE_PIN: u8 = 23;

/// How long (in seconds) the phone rings before giving up and playing the off-hook tone.
pub const RING_TIMEOUT_SECS: u64 = 30;

pub const KNOWN_NUMBERS: [&str; 7] = [
    "0",                                               // Operator
    "7",                                               // Test Number
    "349",                                             // "Fiz"
    "4225",                                            // "Hack"
    "34643664",                                        // "Dingdong",
    "8675309",                                         // the funny
    "47932786463439686262438634258447455587853896846", // "I swear to god if you manage to dial this ill just let you in"
];

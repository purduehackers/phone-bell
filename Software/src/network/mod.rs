pub mod rtc;
pub mod socket;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
pub enum PhoneOutgoingMessage {
    Dial { number: String },
    Hook { state: bool },
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
pub enum PhoneIncomingMessage {
    Ring { state: bool },
    Mute { state: bool },
    PlaySound { sound: Sound },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Sound {
    None,
    Dialtone,
    Ringback,
    Hangup,
}

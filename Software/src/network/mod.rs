pub mod rtc;
pub mod socket;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PhoneOutgoingMessage {
    Dial { number: String },
    Hook { state: bool },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PhoneIncomingMessage {
    Ring { state: bool },
    ClearDial,
}

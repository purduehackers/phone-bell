pub mod config;
pub mod network;
pub mod ui;

pub mod hardware;

use std::str::FromStr;

use network::{rtc::PhoneRTC, socket::PhoneSocket};

use dotenv::dotenv;

use crate::ui::ui_entry;

pub enum PhoneSide {
    Inside,
    Outside,
}

impl FromStr for PhoneSide {
    type Err = ();

    fn from_str(input: &str) -> Result<PhoneSide, Self::Err> {
        match input {
            "Inside" => Ok(PhoneSide::Inside),
            "Outside" => Ok(PhoneSide::Outside),
            _ => Err(()),
        }
    }
}

#[tokio::main]
async fn main() {
    dotenv().ok();

    let phone_side = PhoneSide::from_str(&std::env::var("PHONE_SIDE").unwrap()).unwrap();

    let (mut socket, outgoing_messages, incoming_messages) = PhoneSocket::create(phone_side);

    let (mut rtc, mute_sender) = PhoneRTC::create();

    let websocket_task = tokio::spawn(async move {
        socket.run();
    });

    let webrtc_task = tokio::spawn(async move {
        rtc.run().await;
    });

    ui_entry(outgoing_messages, incoming_messages, mute_sender).await;

    websocket_task.abort();
    webrtc_task.abort();
}

pub mod config;
pub mod network;
pub mod ui;

pub mod hardware;

use std::{str::FromStr, thread};

use hardware::audio::AudioSystem;
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

    let mut websocket_task = tokio::spawn(async move {
        socket.run();
    });

    let mut webrtc_task = tokio::spawn(async move {
        rtc.run().await;
    });

    ui_entry(outgoing_messages, incoming_messages, mute_sender).await;
    // tokio::select! {
    //     _rv_a = (&mut ui_task) => {
    //         websocket_task.abort();
    //         webrtc_task.abort();
    //     }
    //     _rv_b = (&mut websocket_task) => {
    //         ui_task.abort();
    //         webrtc_task.abort();
    //     },
    //     _rv_c = (&mut webrtc_task) => {
    //         ui_task.abort();
    //         websocket_task.abort();
    //     }
    // }
}

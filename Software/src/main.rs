pub mod config;
pub mod network;
pub mod ui;

pub mod hardware;

use std::{str::FromStr, sync::mpsc, thread};

use network::{iroh_voip::PhoneIroh, socket::PhoneSocket};

use dotenv::dotenv;
use tokio::sync::broadcast;

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

    let (mut phone_socket, mut signaling_socket, outgoing_messages, incoming_messages, iroh_addr_sender, peer_addr_receiver) =
        PhoneSocket::create(phone_side);

    let (mut iroh, mute_sender) = PhoneIroh::create(peer_addr_receiver, iroh_addr_sender);

    let phone_control_task = tokio::spawn(async move {
        phone_socket.run().await;
    });

    let signaling_task = tokio::spawn(async move {
        signaling_socket.run().await;
    });

    let iroh_task = tokio::spawn(async move {
        iroh.run().await;
    });

    let (mut socket, outgoing_messages, incoming_messages) = PhoneSocket::create(phone_side);

    let websocket_task = tokio::spawn(async move {
        socket.run();
    });

    let (mute_sender, mute_receiver) = mpsc::channel();

    thread::spawn(move || {
        let mut audio_system = AudioSystem::create();

        loop {
            if let Ok(new_mute) = mute_receiver.try_recv() {
                audio_system.set_mute(new_mute);
            };

            if let Ok(frames) = audio_system.read_next_frames() {
                for frame in frames {
                    let _ = audio_system_mic_sender.send(frame);
                }
            }
            if let Ok(samples) = mixed_output.try_recv() {
                audio_system.write_next_samples(samples.as_slice()).unwrap();
            }
        }
    });

    ui_entry(outgoing_messages, incoming_messages, mute_sender).await;

    phone_control_task.abort();
    signaling_task.abort();
    iroh_task.abort();
}

pub mod config;
pub mod network;
pub mod ui;

pub mod hardware;

use std::{str::FromStr, thread};

use hardware::audio::{AudioMixer, AudioSystem};
use network::{rtc::PhoneRTC, socket::PhoneSocket};

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

    let (mut audio_mixer, mixer_inputs, mixed_output) = AudioMixer::create();

    thread::spawn(move || {
        audio_mixer.run();
    });

    let (mic_sender, _) = broadcast::channel(256);

    let audio_system_mic_sender = mic_sender.clone();

    thread::spawn(move || {
        let mut audio_system = AudioSystem::create();

        loop {
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

    let (mut rtc, mute_sender) = PhoneRTC::create(mixer_inputs, mic_sender);

    let webrtc_task = tokio::spawn(async move {
        rtc.run().await;
    });

    let (mut socket, outgoing_messages, incoming_messages) = PhoneSocket::create(phone_side);

    let websocket_task = tokio::spawn(async move {
        socket.run();
    });

    ui_entry(outgoing_messages, incoming_messages, mute_sender).await;

    webrtc_task.abort();
    websocket_task.abort();
}

pub mod config;
pub mod network;
pub mod ui;

pub mod hardware;

use std::str::FromStr;

use network::{iroh_voip::PhoneIroh, socket::PhoneSocket};

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

    ui_entry(outgoing_messages, incoming_messages, mute_sender).await;

    phone_control_task.abort();
    signaling_task.abort();
    iroh_task.abort();
}

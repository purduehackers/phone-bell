use std::sync::mpsc::{Receiver, Sender};

use crate::{
    config::KNOWN_NUMBERS,
    hardware::{self, audio::AudioSystem, PhoneHardware},
    network::{PhoneIncomingMessage, PhoneOutgoingMessage},
};

pub async fn ui_entry(
    network_sender: Sender<PhoneOutgoingMessage>,
    network_reciever: Receiver<PhoneIncomingMessage>,
    mute_sender: Sender<bool>,
) {
    #[cfg(not(feature = "real"))]
    let mut hardware = hardware::emulated::Hardware::create();
    #[cfg(feature = "real")]
    let mut hardware = hardware::physical::Hardware::create();
    // let audio_system = AudioSystem::create();

    hardware.ring(false);
    hardware.enable_dialing(true);

    let mut in_call = false;
    let _ = mute_sender.send(true);
    let mut last_hook_state = true;

    let mut last_dialed_number = String::from("");

    loop {
        hardware.update();

        let hook_state = hardware.get_hook_state();

        if *hardware.dialed_number() != last_dialed_number && !hardware.dialed_number().is_empty() {
            let mut contains = false;

            for number in KNOWN_NUMBERS {
                if number == hardware.dialed_number() {
                    contains = true;
                }
            }

            if !contains {
                for number in KNOWN_NUMBERS {
                    if number.starts_with(&*hardware.dialed_number()) {
                        contains = true;
                    }
                }

                if !contains {
                    *hardware.dialed_number() = String::from("0");
                }

                contains = !contains;
            }

            if contains {
                hardware.enable_dialing(false);

                if hook_state {
                    hardware.ring(true);
                }

                in_call = true;
                let _ = mute_sender.send(false);

                // ! REMOVE THIS LATER
                let client = reqwest::Client::new();
                let res = client
                    .post("https://api.purduehackers.com/doorbell/ring")
                    .send()
                    .await;

                println!("Calling: {}", hardware.dialed_number());
                let _ = network_sender.send(PhoneOutgoingMessage::Dial {
                    number: hardware.dialed_number().clone(),
                });
            }
        }

        last_dialed_number = hardware.dialed_number().clone();

        if last_hook_state != hook_state {
            let _ = network_sender.send(PhoneOutgoingMessage::Hook { state: hook_state });

            if hook_state {
                if in_call {
                    in_call = false;
                    let _ = mute_sender.send(true);

                    hardware.enable_dialing(true);
                    hardware.dialed_number().clear();

                    println!("Call Ended.");
                }
            } else if in_call {
                hardware.ring(false);
            }
        }

        last_hook_state = hook_state;

        for network_message in network_reciever.try_iter() {
            match network_message {
                PhoneIncomingMessage::Ring { state } => {
                    hardware.ring(state);
                }
                PhoneIncomingMessage::ClearDial => {
                    hardware.dialed_number().clear();
                    hardware.enable_dialing(true);
                }
            }
        }
    }
}

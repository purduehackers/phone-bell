use std::{
    io::Cursor,
    sync::mpsc::{Receiver, Sender},
};

use crate::{
    config::KNOWN_NUMBERS,
    hardware::{self, PhoneHardware},
    network::{PhoneIncomingMessage, PhoneOutgoingMessage},
};
use rodio::{Decoder, OutputStream, Sink, Source};

pub async fn ui_entry(
    network_sender: Sender<PhoneOutgoingMessage>,
    network_reciever: Receiver<PhoneIncomingMessage>,
    mute_sender: Sender<bool>,
) {
    #[cfg(not(feature = "real"))]
    let (mut hardware, ui) = {
        let mut hardware = hardware::emulated::Hardware::create();
        let ui = hardware.take_gui();
        (hardware, ui)
    };
    #[cfg(feature = "real")]
    let mut hardware = hardware::physical::Hardware::create();

    let (_stream, stream_handle) = OutputStream::try_default().unwrap();

    let sink: Sink = Sink::try_new(&stream_handle).unwrap();

    hardware.ring(false);
    hardware.enable_dialing(true);

    let mut in_call = false;
    let _ = mute_sender.send(true);
    let mut last_hook_state = true;

    let mut last_dialed_number = String::from("");

    let mut test_ring = false;

    #[allow(unused_variables)]
    let hnd = tokio::spawn(async move {
        loop {
            hardware.update();

            let hook_state = hardware.get_hook_state();

            if *hardware.dialed_number() != last_dialed_number
                && !hardware.dialed_number().is_empty()
            {
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

                    in_call = true;

                    println!("Calling: {}", hardware.dialed_number());
                    let _ = network_sender.send(PhoneOutgoingMessage::Dial {
                        number: hardware.dialed_number().clone(),
                    });

                    test_ring = hardware.dialed_number() == "7";

                    if hook_state {
                        hardware.ring(true);
                    } else {
                        let _ = mute_sender.send(false);

                        if !test_ring {
                            // ! REMOVE THIS LATER
                            let source = Decoder::new(Cursor::new(include_bytes!(
                                "../assets/doorbell.flac"
                            )))
                            .unwrap();

                            sink.clear();
                            sink.append(source.convert_samples::<f32>());
                            sink.play();

                            // ! REMOVE THIS LATER
                            let client = reqwest::Client::new();
                            let _ = client
                                .post("https://api.purduehackers.com/doorbell/ring")
                                .send()
                                .await;
                        } else {
                            let source = Decoder::new(Cursor::new(include_bytes!(
                                "../assets/dial_test.flac"
                            )))
                            .unwrap();

                            sink.clear();
                            sink.append(source.convert_samples::<f32>());
                            sink.play();
                        }
                    }
                }
            }

            last_dialed_number = hardware.dialed_number().clone();

            if last_hook_state != hook_state {
                let _ = network_sender.send(PhoneOutgoingMessage::Hook { state: hook_state });

                if hook_state {
                    sink.clear();

                    if in_call {
                        in_call = false;
                        let _ = mute_sender.send(true);

                        hardware.enable_dialing(true);
                        hardware.dialed_number().clear();

                        println!("Call Ended.");
                    }
                } else if in_call {
                    hardware.ring(false);

                    let _ = mute_sender.send(false);

                    if !test_ring {
                        // ! REMOVE THIS LATER
                        let source =
                            Decoder::new(Cursor::new(include_bytes!("../assets/doorbell.flac")))
                                .unwrap();

                        sink.clear();
                        sink.append(source.convert_samples::<f32>());
                        sink.play();

                        // ! REMOVE THIS LATER
                        let client = reqwest::Client::new();
                        let _ = client
                            .post("https://api.purduehackers.com/doorbell/ring")
                            .send()
                            .await;
                    } else {
                        let source =
                            Decoder::new(Cursor::new(include_bytes!("../assets/dial_test.flac")))
                                .unwrap();

                        sink.clear();
                        sink.append(source.convert_samples::<f32>());
                        sink.play();
                    }
                } else {
                    let source =
                        Decoder::new_looped(Cursor::new(include_bytes!("../assets/dialtone.flac")))
                            .unwrap();

                    sink.clear();
                    sink.append(source.convert_samples::<f32>());
                    sink.play();
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
    });

    #[cfg(feature = "real")]
    {
        hnd.await;
    }
    #[cfg(not(feature = "real"))]
    {
        ui.go();
    }
}

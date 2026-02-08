use std::{
    io::Cursor,
    sync::mpsc::{Receiver, Sender},
};

use crate::{
    config::KNOWN_NUMBERS,
    hardware::{self, PhoneHardware},
    network::{PhoneIncomingMessage, PhoneOutgoingMessage, Sound},
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
    let mut ringing = false;
    let _ = mute_sender.send(true);
    let mut last_hook_state = true;

    let mut last_dialed_number = String::from("");

    let mut silent_ring = false;

    #[allow(unused_variables)]
    let hnd = tokio::spawn(async move {
        loop {
            hardware.update();

            let hook_state = hardware.get_hook_state();

            if *hardware.dialed_number() != last_dialed_number
                && !hardware.dialed_number().is_empty()
            {
                if in_call {
                    // In-call DTMF: forward the latest digit to the server
                    if let Some(digit) = hardware.dialed_number().chars().last() {
                        println!("In-call dial: {}", digit);
                        let _ = network_sender.send(PhoneOutgoingMessage::Dial {
                            number: digit.to_string(),
                        });
                    }
                    hardware.dialed_number().clear();
                } else {
                    // Normal number matching for initiating a call
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

                        silent_ring = hardware.dialed_number() == "7";

                        if hook_state {
                            hardware.ring(true);
                        } else {
                            // Unmute immediately; server will also send Mute(false)
                            let _ = mute_sender.send(false);
                            sink.clear();

                            if !silent_ring {
                                // ! REMOVE THIS LATER
                                tokio::spawn(async {
                                    let client = reqwest::Client::new();
                                    let _ = client
                                        .post("https://api.purduehackers.com/doorbell/ring")
                                        .send()
                                        .await;
                                });
                            }
                        }
                    }
                }
            }

            last_dialed_number = hardware.dialed_number().clone();

            if last_hook_state != hook_state {
                let _ = network_sender.send(PhoneOutgoingMessage::Hook { state: hook_state });

                if hook_state {
                    // Hung up — mute immediately; server will also send Mute(true)
                    sink.clear();
                    let _ = mute_sender.send(true);

                    if in_call || ringing {
                        in_call = false;
                        ringing = false;

                        hardware.enable_dialing(true);
                        hardware.dialed_number().clear();

                        println!("Call Ended.");
                    }
                } else if ringing {
                    // Answering an incoming call from the server
                    println!("Answering incoming call");
                    hardware.ring(false);
                    ringing = false;
                    in_call = true;
                    sink.clear();
                    let _ = mute_sender.send(false);
                    hardware.enable_dialing(true);
                    hardware.dialed_number().clear();
                } else if in_call {
                    // Picking up after on-hook dial
                    hardware.ring(false);
                    sink.clear();
                    let _ = mute_sender.send(false);

                    if !silent_ring {
                        // ! REMOVE THIS LATER
                        let client = reqwest::Client::new();
                        let _ = client
                            .post("https://api.purduehackers.com/doorbell/ring")
                            .send()
                            .await;
                    }
                    // Server will send Mute(false) and PlaySound(Ringback)
                } else {
                    // Fresh pickup, no call — play dialtone immediately
                    // (server will also send PlaySound(Dialtone))
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
                        println!("Ring: {}", state);
                        hardware.ring(state);
                        ringing = state;
                    }
                    PhoneIncomingMessage::ClearDial => {
                        hardware.dialed_number().clear();
                        hardware.enable_dialing(true);
                    }
                    PhoneIncomingMessage::PlaySound { sound } => {
                        println!("PlaySound: {:?}", sound);
                        match sound {
                            Sound::Dialtone => {
                                let source = Decoder::new_looped(Cursor::new(
                                    include_bytes!("../assets/dialtone.flac"),
                                ))
                                .unwrap();
                                sink.clear();
                                sink.append(source.convert_samples::<f32>());
                                sink.play();
                            }
                            Sound::None => {
                                sink.clear();
                                // Call connected — re-enable dialing for in-call DTMF
                                hardware.enable_dialing(true);
                                hardware.dialed_number().clear();
                            }
                            Sound::Ringback | Sound::Hangup => {
                                // TODO: add ringback and hangup sound assets
                                sink.clear();
                            }
                        }
                    }
                    PhoneIncomingMessage::Mute { state } => {
                        println!("Mute: {}", state);
                        let _ = mute_sender.send(state);
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

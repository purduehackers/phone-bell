use std::{
    io::Cursor,
    sync::mpsc::{Receiver, Sender},
    time::Instant,
};

use crate::{
    config::{KNOWN_NUMBERS, RING_TIMEOUT_SECS},
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
    let mut pending_dial: Option<String> = None;
    let mut ring_start: Option<Instant> = None;

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
                    // If number isn't valid, just assume 0
                    if !KNOWN_NUMBERS.contains(&hardware.dialed_number().as_str()) {
                        *hardware.dialed_number() = String::from("0");
                    }

                    hardware.enable_dialing(false);

                    if hook_state {
                        // On-hook dial: defer call initiation until pickup
                        pending_dial = Some(hardware.dialed_number().clone());
                        hardware.ring(true);
                    } else {
                        in_call = true;

                        println!("Calling: {}", hardware.dialed_number());
                        let _ = network_sender.send(PhoneOutgoingMessage::Dial {
                            number: hardware.dialed_number().clone(),
                        });

                        // Unmute immediately; server will also send Mute(false)
                        let _ = mute_sender.send(false);
                        sink.clear();
                        ring_start = Some(Instant::now());
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
                        println!("Call Ended.");
                    }

                    in_call = false;
                    ringing = false;
                    ring_start = None;
                    pending_dial = None;
                    hardware.enable_dialing(true);
                    hardware.dialed_number().clear();
                } else if ringing {
                    // Answering an incoming call from the server
                    println!("Answering incoming call");
                    hardware.ring(false);
                    ringing = false;
                    ring_start = None;
                    in_call = true;
                    sink.clear();
                    let _ = mute_sender.send(false);
                    hardware.enable_dialing(true);
                    hardware.dialed_number().clear();
                } else if let Some(number) = pending_dial.take() {
                    // Picking up after on-hook dial: now initiate the call
                    hardware.ring(false);
                    in_call = true;
                    println!("Calling: {}", number);
                    let _ = network_sender.send(PhoneOutgoingMessage::Dial { number });
                    let _ = mute_sender.send(false);
                    sink.clear();
                    ring_start = Some(Instant::now());
                } else if in_call {
                    // Already in call, just unmute
                    hardware.ring(false);
                    sink.clear();
                    let _ = mute_sender.send(false);
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

            // Ring timeout: no one picked up — play off-hook tone and end the call
            if let Some(start) = ring_start {
                if start.elapsed().as_secs() >= RING_TIMEOUT_SECS {
                    println!("Ring timeout — no answer");
                    hardware.ring(false);
                    ringing = false;
                    in_call = false;
                    ring_start = None;

                    // Tell the server we're hanging up so the other side stops ringing
                    let _ = network_sender.send(PhoneOutgoingMessage::Hook { state: true });

                    // Play off-hook tone
                    if let Ok(source) = Decoder::new_looped(Cursor::new(
                        include_bytes!("../assets/off_hook.flac"),
                    )) {
                        sink.clear();
                        sink.append(source.convert_samples::<f32>());
                        sink.play();
                    } else {
                        eprintln!("Failed to decode off_hook.flac");
                    }
                }
            }

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
                                let source = Decoder::new_looped(Cursor::new(include_bytes!(
                                    "../assets/dialtone.flac"
                                )))
                                .unwrap();
                                sink.clear();
                                sink.append(source.convert_samples::<f32>());
                                sink.play();
                            }
                            Sound::None => {
                                sink.clear();
                                ring_start = None;
                                // Re-enable dialing for in-call DTMF
                                hardware.enable_dialing(true);
                                hardware.dialed_number().clear();
                                // If phone is still off-hook but not in a call,
                                // play off-hook tone (e.g. ring timeout, remote hangup)
                                if !in_call && !hook_state {
                                    if let Ok(source) = Decoder::new_looped(Cursor::new(
                                        include_bytes!("../assets/off_hook.flac"),
                                    )) {
                                        sink.append(source.convert_samples::<f32>());
                                        sink.play();
                                    }
                                }
                            }
                            Sound::Ringback => {
                                let source = Decoder::new_looped(Cursor::new(
                                    include_bytes!("../assets/ringback.flac"),
                                ))
                                .unwrap();
                                sink.clear();
                                sink.append(source.convert_samples::<f32>());
                                sink.play();
                            }
                            Sound::Hangup => {
                                in_call = false;
                                ring_start = None;
                                sink.clear();
                                if let Ok(hangup) = Decoder::new(Cursor::new(
                                    include_bytes!("../assets/hangup.flac"),
                                )) {
                                    sink.append(hangup.convert_samples::<f32>());
                                }
                                // Follow hangup with looped off-hook tone
                                if let Ok(off_hook) = Decoder::new_looped(Cursor::new(
                                    include_bytes!("../assets/off_hook.flac"),
                                )) {
                                    sink.append(off_hook.convert_samples::<f32>());
                                }
                                sink.play();
                            }
                            Sound::DoorOpen => {
                                if let Ok(source) = Decoder::new(Cursor::new(
                                    include_bytes!("../assets/door_open.flac"),
                                )) {
                                    sink.append(source.convert_samples::<f32>());
                                    sink.play();
                                }
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

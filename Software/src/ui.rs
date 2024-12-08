use std::{
    io::Cursor,
    sync::mpsc::{Receiver, Sender},
};

use crate::{
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

    let _ = mute_sender.send(true);

    let mut last_hook_state = true;

    #[allow(unused_variables)]
    let ui_process_join_handle = tokio::spawn(async move {
        loop {
            hardware.update();

            if !(*hardware.dialed_number()).is_empty() {
                let _ = network_sender.send(PhoneOutgoingMessage::Dial {
                    number: hardware.dialed_number().clone(),
                });

                *hardware.dialed_number() = String::from("");
            }

            if hardware.get_hook_state() != last_hook_state {
                last_hook_state = hardware.get_hook_state();

                let _ = network_sender.send(PhoneOutgoingMessage::Hook {
                    state: last_hook_state,
                });
            }

            while let Ok(network_message) = network_reciever.try_recv() {
                println!("Network Message: {:?}", network_message);

                match network_message {
                    PhoneIncomingMessage::Ring { state } => {
                        hardware.ring(state);
                    }
                    PhoneIncomingMessage::Mute { state } => {
                        let _ = mute_sender.send(state);
                    }
                    PhoneIncomingMessage::PlaySound { sound } => match sound {
                        Sound::None => {
                            sink.clear();
                            sink.pause();
                        }
                        Sound::Dialtone => {
                            let source = Decoder::new_looped(Cursor::new(include_bytes!(
                                "../assets/dialtone.flac"
                            )))
                            .unwrap();

                            sink.clear();
                            sink.append(source.convert_samples::<f32>());
                            sink.play();
                        }
                        Sound::Ringback => {
                            let source = Decoder::new_looped(Cursor::new(include_bytes!(
                                "../assets/ringback.flac"
                            )))
                            .unwrap();

                            sink.clear();
                            sink.append(source.convert_samples::<f32>());
                            sink.play();
                        }
                        Sound::Hangup => {
                            let source = Decoder::new_looped(Cursor::new(include_bytes!(
                                "../assets/hangup.flac"
                            )))
                            .unwrap();

                            sink.clear();
                            sink.append(source.convert_samples::<f32>());
                            sink.play();
                        }
                    },
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

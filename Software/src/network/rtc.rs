use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicI64, Ordering},
        mpsc::{self},
        Arc,
    },
    thread,
};

use bytes::Bytes;
use opus::{Channels, Decoder, Encoder};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, watch};
use uuid::Uuid;
use webrtc::{
    api::{
        interceptor_registry::register_default_interceptors,
        media_engine::{MediaEngine, MIME_TYPE_OPUS},
        APIBuilder, API,
    },
    ice_transport::{
        ice_candidate::{RTCIceCandidate, RTCIceCandidateInit},
        ice_server::RTCIceServer,
    },
    interceptor::registry::Registry,
    peer_connection::{
        configuration::RTCConfiguration, peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription, RTCPeerConnection,
    },
    rtp::{
        codecs::opus::OpusPayloader,
        packetizer::{new_packetizer, Packetizer},
        sequence::new_random_sequencer,
    },
    rtp_transceiver::rtp_codec::{RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType},
    track::track_local::{
        track_local_static_rtp::TrackLocalStaticRTP, TrackLocal, TrackLocalWriter,
    },
};

use crate::{config::SAMPLE_RATE, hardware::audio::MixerMessage};

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
pub enum SignalingMessage {
    Join {
        from: Uuid,
    },
    JoinAck {
        from: Uuid,
    },
    ICEOffer {
        offer: RTCSessionDescription,
        from: Uuid,
        to: Uuid,
    },
    ICEAnswer {
        answer: RTCSessionDescription,
        from: Uuid,
        to: Uuid,
    },
    ICECandidate {
        candidate: RTCIceCandidateInit,
        from: Uuid,
        to: Uuid,
    },
    Leave {
        from: Uuid,
    },
}

pub struct PhoneRTC {
    signaling_socket: Option<
        websocket::client::sync::Client<
            websocket::stream::sync::TlsStream<websocket::stream::sync::TcpStream>,
        >,
    >,
    webrtc_api: API,
    mute_receiver: mpsc::Receiver<bool>,
    peer_connections: HashMap<Uuid, RTCPeerConnection>,
    mixer_out: mpsc::Sender<MixerMessage>,
    mic_in: broadcast::Sender<Vec<f32>>,
    id: Uuid,
    muted: bool,
}

impl PhoneRTC {
    pub fn create(
        mixer_out: mpsc::Sender<MixerMessage>,
        mic_in: broadcast::Sender<Vec<f32>>,
    ) -> (PhoneRTC, mpsc::Sender<bool>) {
        let (mute_sender, mute_receiver) = mpsc::channel();

        let mut m = MediaEngine::default();

        m.register_codec(
            RTCRtpCodecParameters {
                capability: RTCRtpCodecCapability {
                    mime_type: MIME_TYPE_OPUS.to_owned(),
                    ..Default::default()
                },
                payload_type: 120,
                ..Default::default()
            },
            RTPCodecType::Audio,
        )
        .unwrap();

        let mut registry = Registry::new();

        registry = register_default_interceptors(registry, &mut m).unwrap();

        let webrtc_api = APIBuilder::new()
            .with_media_engine(m)
            .with_interceptor_registry(registry)
            .build();

        let mut socket = PhoneRTC {
            signaling_socket: None,
            webrtc_api,
            mute_receiver,
            peer_connections: HashMap::new(),
            mixer_out,
            mic_in,
            id: Uuid::new_v4(),
            muted: true,
        };

        socket.connect();

        (socket, mute_sender)
    }

    fn connect(&mut self) {
        if self.signaling_socket.is_some() {
            return;
        }

        let Ok(mut websocket_client_builder) =
            websocket::ClientBuilder::new("wss://api.purduehackers.com/phonebell/signaling")
        else {
            return;
        };

        let Ok(mut websocket_client) = websocket_client_builder.connect_secure(Option::None) else {
            return;
        };

        let Ok(_) = websocket_client.send_message(&websocket::Message::text("gm!")) else {
            return;
        };

        let Ok(message_string) = serde_json::to_string(&SignalingMessage::Join { from: self.id })
        else {
            return;
        };

        let Ok(_) = websocket_client.send_message(&websocket::Message::text(message_string)) else {
            return;
        };

        println!("webrtc tx: {:?}", SignalingMessage::Join { from: self.id });

        self.signaling_socket = Some(websocket_client);
    }

    pub async fn run(&mut self) {
        let (ice_candidate_channel_sender, ice_candidate_channel_receiver) =
            mpsc::channel::<(RTCIceCandidate, Uuid)>();
        let (connection_change_channel_sender, connection_change_channel_receiver) =
            mpsc::channel::<(RTCPeerConnectionState, Uuid)>();

        let (signaling_message_sender, signaling_message_receiver) =
            mpsc::channel::<SignalingMessage>();
        let (signaling_pong_sender, signaling_pong_receiver) = mpsc::channel::<Vec<u8>>();

        let (mute_sender, mute_receiver) = watch::channel(true);

        loop {
            if self.signaling_socket.is_none() {
                self.connect();
            }

            if let Ok(mute) = self.mute_receiver.try_recv() {
                self.muted = mute;

                let _ = mute_sender.send(mute);
            }

            if let Ok((connection_state, from)) = connection_change_channel_receiver.try_recv() {
                if connection_state == RTCPeerConnectionState::Disconnected
                    || connection_state == RTCPeerConnectionState::Failed
                {
                    if let Some(peer_connection) = self.peer_connections.remove(&from) {
                        let _ = peer_connection.close().await;
                    }
                }
            }

            if let Some(signaling_socket) = &mut self.signaling_socket {
                let mut should_shutdown = false;

                'message_iterate: {
                    if let Ok(message) = (*signaling_socket).recv_message() {
                        match message {
                            websocket::OwnedMessage::Text(data) => {
                                let Ok(message): Result<SignalingMessage, serde_json::Error> =
                                    serde_json::from_str(&data)
                                else {
                                    break 'message_iterate;
                                };

                                println!("webrtc rx {:?}", message);

                                match message {
                                    SignalingMessage::Join { from } => {
                                        if from != self.id {
                                            println!("Join from: {} {}", from, self.id);

                                            let signaling_message_sender_clone =
                                                signaling_message_sender.clone();
                                            let from_clone = self.id;

                                            thread::spawn(move || {
                                                let _ = signaling_message_sender_clone.send(
                                                    SignalingMessage::JoinAck { from: from_clone },
                                                );
                                            });
                                        }
                                    }
                                    SignalingMessage::JoinAck { from } => {
                                        if from != self.id
                                            && !self.peer_connections.contains_key(&from)
                                        {
                                            println!("JoinAck from: {} {}", from, self.id);

                                            let config = RTCConfiguration {
                                                ice_servers: vec![RTCIceServer {
                                                    urls: vec![
                                                        "stun:stun.l.google.com:19302".to_owned()
                                                    ],
                                                    ..Default::default()
                                                }],
                                                ..Default::default()
                                            };

                                            let Ok(new_peer_connection) =
                                                self.webrtc_api.new_peer_connection(config).await
                                            else {
                                                break 'message_iterate;
                                            };

                                            let Ok(_) = new_peer_connection
                                                .add_transceiver_from_kind(
                                                    RTPCodecType::Audio,
                                                    None,
                                                )
                                                .await
                                            else {
                                                break 'message_iterate;
                                            };

                                            if !setup_peer_connection_audio(
                                                &self.mixer_out,
                                                &self.mic_in,
                                                &new_peer_connection,
                                                &mute_receiver,
                                            )
                                            .await
                                            {
                                                break 'message_iterate;
                                            }

                                            let Ok(offer) =
                                                &(new_peer_connection.create_offer(None).await)
                                            else {
                                                break 'message_iterate;
                                            };

                                            let Ok(_) = new_peer_connection
                                                .set_local_description(offer.clone())
                                                .await
                                            else {
                                                break 'message_iterate;
                                            };

                                            let new_connection_change_channel_sender =
                                                connection_change_channel_sender.clone();

                                            new_peer_connection.on_peer_connection_state_change(
                                                Box::new(move |connection_state| {
                                                    println!(
                                                        "PeerConnection to {} changed to {}",
                                                        from, connection_state
                                                    );

                                                    let _ = new_connection_change_channel_sender
                                                        .send((connection_state, from));
                                                    Box::pin(async {})
                                                }),
                                            );

                                            self.peer_connections.insert(from, new_peer_connection);

                                            let _ = signaling_message_sender.send(
                                                SignalingMessage::ICEOffer {
                                                    offer: offer.clone(),
                                                    from: self.id,
                                                    to: from,
                                                },
                                            );
                                        }
                                    }
                                    SignalingMessage::ICEOffer { offer, from, to } => {
                                        if from != self.id
                                            && to == self.id
                                            && !self.peer_connections.contains_key(&from)
                                        {
                                            println!("ICEOffer from: {}", from);

                                            let config = RTCConfiguration {
                                                ice_servers: vec![RTCIceServer {
                                                    urls: vec![
                                                        "stun:stun.l.google.com:19302".to_owned()
                                                    ],
                                                    ..Default::default()
                                                }],
                                                ..Default::default()
                                            };

                                            let Ok(new_peer_connection) =
                                                self.webrtc_api.new_peer_connection(config).await
                                            else {
                                                break 'message_iterate;
                                            };

                                            let Ok(_) = new_peer_connection
                                                .add_transceiver_from_kind(
                                                    RTPCodecType::Audio,
                                                    None,
                                                )
                                                .await
                                            else {
                                                break 'message_iterate;
                                            };

                                            if !setup_peer_connection_audio(
                                                &self.mixer_out,
                                                &self.mic_in,
                                                &new_peer_connection,
                                                &mute_receiver,
                                            )
                                            .await
                                            {
                                                break 'message_iterate;
                                            }

                                            let Ok(_) = new_peer_connection
                                                .set_remote_description(offer)
                                                .await
                                            else {
                                                break 'message_iterate;
                                            };

                                            let Ok(answer) =
                                                &(new_peer_connection.create_answer(None).await)
                                            else {
                                                break 'message_iterate;
                                            };

                                            let Ok(_) = new_peer_connection
                                                .set_local_description(answer.clone())
                                                .await
                                            else {
                                                break 'message_iterate;
                                            };

                                            let new_ice_candidate_channel_sender =
                                                ice_candidate_channel_sender.clone();

                                            new_peer_connection.on_ice_candidate(Box::new(
                                                move |candidate_option| {
                                                    if let Some(candidate) = candidate_option {
                                                        let _ = new_ice_candidate_channel_sender
                                                            .send((candidate, from));
                                                    }
                                                    Box::pin(async {})
                                                },
                                            ));

                                            let new_connection_change_channel_sender =
                                                connection_change_channel_sender.clone();

                                            new_peer_connection.on_peer_connection_state_change(
                                                Box::new(move |connection_state| {
                                                    println!(
                                                        "PeerConnection to {} changed to {}",
                                                        from, connection_state
                                                    );

                                                    let _ = new_connection_change_channel_sender
                                                        .send((connection_state, from));
                                                    Box::pin(async {})
                                                }),
                                            );

                                            self.peer_connections.insert(from, new_peer_connection);

                                            let _ = signaling_message_sender.send(
                                                SignalingMessage::ICEAnswer {
                                                    answer: answer.clone(),
                                                    from: self.id,
                                                    to: from,
                                                },
                                            );
                                        }
                                    }
                                    SignalingMessage::ICEAnswer { answer, from, to } => {
                                        if from != self.id && to == self.id {
                                            if let Some(peer_connection) =
                                                self.peer_connections.get(&from)
                                            {
                                                println!("ICEAnswer from: {}", from);

                                                let Ok(_) = peer_connection
                                                    .set_remote_description(answer)
                                                    .await
                                                else {
                                                    break 'message_iterate;
                                                };

                                                let new_ice_candidate_channel_sender =
                                                    ice_candidate_channel_sender.clone();

                                                peer_connection.on_ice_candidate(Box::new(
                                                    move |candidate_option| {
                                                        if let Some(candidate) = candidate_option {
                                                            let _ =
                                                                new_ice_candidate_channel_sender
                                                                    .send((candidate, from));
                                                        }
                                                        Box::pin(async {})
                                                    },
                                                ));
                                            }
                                        }
                                    }
                                    SignalingMessage::ICECandidate {
                                        candidate,
                                        from,
                                        to,
                                    } => {
                                        if from != self.id && to == self.id {
                                            if let Some(peer_connection) =
                                                self.peer_connections.get(&from)
                                            {
                                                println!("ICEAnswer from: {}", from);

                                                let Ok(_) = peer_connection
                                                    .add_ice_candidate(candidate)
                                                    .await
                                                else {
                                                    break 'message_iterate;
                                                };
                                            }
                                        }
                                    }
                                    SignalingMessage::Leave { from } => {
                                        if from != self.id {
                                            println!("Leave from: {}", from);

                                            if let Some(peer_connection) =
                                                self.peer_connections.remove(&from)
                                            {
                                                let _ = peer_connection.close().await;
                                            }
                                        }
                                    }
                                }
                            }
                            websocket::OwnedMessage::Binary(_) => {}
                            websocket::OwnedMessage::Close(_) => {
                                let _ = signaling_socket.shutdown();
                                should_shutdown = true;

                                break 'message_iterate;
                            }
                            websocket::OwnedMessage::Ping(data) => {
                                let _ = signaling_pong_sender.send(data);
                            }
                            websocket::OwnedMessage::Pong(_) => {}
                        }
                    }
                }

                if should_shutdown {
                    self.signaling_socket = None;
                } else {
                    for (candidate, from) in ice_candidate_channel_receiver.try_iter() {
                        if let Ok(candidate_init) = candidate.to_json() {
                            let _ = signaling_message_sender.send(SignalingMessage::ICECandidate {
                                candidate: candidate_init,
                                from: self.id,
                                to: from,
                            });
                        }
                    }

                    'sender_loop: for message in signaling_message_receiver.try_iter() {
                        println!("webrtc pre tx {:?}", message);

                        let Ok(message_string) = serde_json::to_string(&message) else {
                            continue 'sender_loop;
                        };

                        let _ = (*signaling_socket)
                            .send_message(&websocket::Message::text(message_string));

                        println!("webrtc tx {:?}", message);
                    }

                    for data in signaling_pong_receiver.try_iter() {
                        let _ = (*signaling_socket).send_message(&websocket::Message::pong(data));
                    }
                }
            }
        }
    }
}

static CHANNEL_INDEXER: AtomicI64 = AtomicI64::new(0);

async fn setup_peer_connection_audio(
    mixer_out: &mpsc::Sender<MixerMessage>,
    mic_in: &broadcast::Sender<Vec<f32>>,
    new_peer_connection: &RTCPeerConnection,
    mute_receiver: &watch::Receiver<bool>,
) -> bool {
    const SAMPLE_RATE_PER_MILLISECOND: f32 = (SAMPLE_RATE / 1000) as f32;

    const FRAME_LENGTH_1200: usize = (SAMPLE_RATE_PER_MILLISECOND * 60.0) as usize;

    let output_track = Arc::new(TrackLocalStaticRTP::new(
        RTCRtpCodecCapability {
            mime_type: MIME_TYPE_OPUS.to_owned(),
            clock_rate: SAMPLE_RATE,
            channels: 1,
            ..Default::default()
        },
        "track-audio".to_string(),
        "webrtc-rs".to_owned(),
    ));

    let Ok(rtcp_sender) = new_peer_connection
        .add_track(Arc::clone(&output_track) as Arc<dyn TrackLocal + Send + Sync>)
        .await
    else {
        return false;
    };

    let mut mic_receiver = mic_in.subscribe();
    let mute_receiver_encoder = mute_receiver.clone();

    tokio::spawn(async move {
        let Ok(mut encoder) = Encoder::new(SAMPLE_RATE, Channels::Mono, opus::Application::Voip)
        else {
            return Err(());
        };

        let mut mute_receiver_encoder = mute_receiver_encoder.clone();

        let audio_send_task = tokio::spawn(async move {
            let payloader = OpusPayloader;
            let sequencer = new_random_sequencer();
            let mut packetizer = new_packetizer(
                1276,
                120,
                69,
                Box::new(payloader),
                Box::new(sequencer),
                SAMPLE_RATE,
            );

            loop {
                let Ok(next_audio_frames) = mic_receiver.recv().await else {
                    continue;
                };

                let mute = *mute_receiver_encoder.borrow_and_update();

                let next_audio_frames_processed = next_audio_frames
                    .into_iter()
                    .map(|sample| if mute { 0.0 } else { sample })
                    .collect::<Vec<f32>>();

                let encode_result = encoder.encode_vec_float(
                    next_audio_frames_processed.as_slice(),
                    next_audio_frames_processed.len(),
                );

                let Ok(next_audio_frames) = encode_result else {
                    continue;
                };

                let number_frames = next_audio_frames.len();

                let Ok(rtp_packets) =
                    packetizer.packetize(&Bytes::from(next_audio_frames), number_frames as u32)
                else {
                    continue;
                };

                for rtp_packet in rtp_packets {
                    let _ = output_track.write_rtp(&rtp_packet).await;
                }
            }
        });

        let mut rtcp_buf = vec![0u8; 1500];

        while let Ok((_, _)) = rtcp_sender.read(&mut rtcp_buf).await {}

        audio_send_task.abort();

        Result::<(), ()>::Ok(())
    });

    let mixer_sender = mixer_out.clone();
    let mute_receiver_decoder = mute_receiver.clone();

    new_peer_connection.on_track(Box::new(move |remote_track, rtcp_receiver, _| {
        let channel_number = CHANNEL_INDEXER.fetch_add(1, Ordering::SeqCst);

        let Ok(mut decoder) = Decoder::new(SAMPLE_RATE, Channels::Mono) else {
            return Box::pin(async {});
        };

        let _ = mixer_sender.send(MixerMessage::Open(channel_number));

        let mixer_sender_loop = mixer_sender.clone();
        let mixer_sender_termination = mixer_sender_loop.clone();
        let mut mute_receiver_decoder = mute_receiver_decoder.clone();

        tokio::spawn(async move {
            let audio_receive_task = tokio::spawn(async move {
                loop {
                    let Ok((rtp_packet, _)) = remote_track.read_rtp().await else {
                        continue;
                    };

                    let sequence_number = rtp_packet.header.sequence_number;

                    let mut audio_data: [f32; FRAME_LENGTH_1200] = [0.0; FRAME_LENGTH_1200];

                    let decode_result =
                        decoder.decode_float(&rtp_packet.payload, &mut audio_data, false);

                    let Ok(decode_length) = decode_result else {
                        continue;
                    };

                    let mute = *mute_receiver_decoder.borrow_and_update();

                    let _ = mixer_sender_loop.send(MixerMessage::Samples(
                        channel_number,
                        sequence_number,
                        audio_data
                            .to_vec()
                            .drain(0..decode_length)
                            .map(|sample| if mute { 0.0 } else { sample })
                            .collect(),
                    ));
                }
            });

            let mut rtcp_buf = vec![0u8; 1500];

            while let Ok((_, _)) = rtcp_receiver.read(&mut rtcp_buf).await {}

            audio_receive_task.abort();

            let _ = mixer_sender_termination.send(MixerMessage::Close(channel_number));
        });

        Box::pin(async {})
    }));

    true
}

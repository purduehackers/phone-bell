use std::{
    collections::HashMap,
    sync::{
        mpsc::{channel, Receiver, Sender},
        Arc,
    },
    thread,
};

use serde::{Deserialize, Serialize};
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
    rtp_transceiver::rtp_codec::{RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType},
    track::track_local::{
        track_local_static_rtp::TrackLocalStaticRTP, TrackLocal, TrackLocalWriter,
    },
};

use crate::hardware::audio::AudioSystem;

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
    mute_receiver: Receiver<bool>,
    peer_connections: HashMap<Uuid, RTCPeerConnection>,
    audio_streams: HashMap<Uuid, Arc<TrackLocalStaticRTP>>,
    id: Uuid,
    muted: bool,
}

impl PhoneRTC {
    pub fn create() -> (PhoneRTC, Sender<bool>) {
        let (mute_sender, mute_receiver) = channel();

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
            audio_streams: HashMap::new(),
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
            channel::<(RTCIceCandidate, Uuid)>();
        let (connection_change_channel_sender, connection_change_channel_receiver) =
            channel::<(RTCPeerConnectionState, Uuid)>();

        let (signaling_message_sender, signaling_message_receiver) = channel::<SignalingMessage>();
        let (signaling_pong_sender, signaling_pong_receiver) = channel::<Vec<u8>>();

        loop {
            if self.signaling_socket.is_none() {
                self.connect();
            }

            if let Ok(mute) = self.mute_receiver.try_recv() {
                self.muted = mute;

                // TODO:
                // if (this.stream) {
                // 	for (let track of this.stream.getAudioTracks()) {
                // 		track.enabled = !state;
                // 	}
                // }

                // for (let audioStream of Object.values(this.audioStreams)) {
                // 	audioStream.muted = state;
                // }
            }

            if let Ok((connection_state, from)) = connection_change_channel_receiver.try_recv() {
                if connection_state == RTCPeerConnectionState::Disconnected
                    || connection_state == RTCPeerConnectionState::Failed
                {
                    if let Some(_) = self.audio_streams.remove(&from) {
                        // TODO: Clean up audio
                    }

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

                                            if !setup_peer_connection_audio(&new_peer_connection)
                                                .await
                                            {
                                                break 'message_iterate;
                                            }

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
                                                .set_remote_description(offer)
                                                .await
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

                                            if !setup_peer_connection_audio(&new_peer_connection)
                                                .await
                                            {
                                                break 'message_iterate;
                                            }

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

                                            if let Some(_) = self.audio_streams.remove(&from) {
                                                // TODO: Clean up audio
                                            }

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
                                // let _ =
                                //     (*signaling_socket).send_message(&websocket::Message::pong(data));
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

async fn setup_peer_connection_audio(new_peer_connection: &RTCPeerConnection) -> bool {
    // TODO:
    // if (this.stream) {
    // 	for (let track of this.stream.getAudioTracks()) {
    // 		peerConnection.addTrack(track, this.stream);
    // 	}
    // }

    // peerConnection.addEventListener("track", async (event) => {
    // 	const [remoteStream] = event.streams;

    // 	let newAudioElement = document.createElement("audio");

    // 	newAudioElement.srcObject = remoteStream;
    // 	newAudioElement.autoplay = true;

    // 	this.audioStreams[target] = newAudioElement;
    // });

    let output_track = Arc::new(TrackLocalStaticRTP::new(
        RTCRtpCodecCapability {
            mime_type: MIME_TYPE_OPUS.to_owned(),
            ..Default::default()
        },
        "track-audio".to_string(),
        "webrtc-rs".to_owned(),
    ));

    let Ok(rtp_sender) = new_peer_connection
        .add_track(Arc::clone(&output_track) as Arc<dyn TrackLocal + Send + Sync>)
        .await
    else {
        return false;
    };
    tokio::spawn(async move {
        let mut rtcp_buf = vec![0u8; 1500];

        // output_track.write_rtp(p);

        while let Ok((_, _)) = rtp_sender.read(&mut rtcp_buf).await {}

        println!("audio rtp_sender.read loop exit");

        Result::<(), ()>::Ok(())
    });

    new_peer_connection.on_track(Box::new(move |a, b, c| {
        println!("remote track! {:?} {:?} {:?}", a, b, c);

        b.read_rtcp();

        Box::pin(async {})
    }));

    true
}

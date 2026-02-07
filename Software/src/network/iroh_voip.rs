use std::sync::mpsc::{channel, Receiver, Sender};

use anyhow::Result;
use audiopus::{coder::Decoder, coder::Encoder, packet::Packet, Channels, MutSignals, SampleRate, Application};
use iroh::{endpoint::Connection, Endpoint};

use crate::hardware::audio::AudioSystemMarshaller;

pub const PHONEBELL_ALPN: &[u8] = b"phonebell/voip/1";

const OPUS_SAMPLE_RATE: SampleRate = SampleRate::Hz48000;
const OPUS_CHANNELS: Channels = Channels::Mono;
const OPUS_FRAME_SIZE: usize = 960; // 20ms at 48kHz

pub struct PhoneIroh {
    endpoint: Option<Endpoint>,
    active_connection: Option<Connection>,
    mute_receiver: Receiver<bool>,
    peer_addr_receiver: Receiver<String>,
    our_addr_sender: Sender<String>,
    muted: bool,
    mic_buffer: Vec<f32>,
}

impl PhoneIroh {
    pub fn create(
        peer_addr_receiver: Receiver<String>,
        our_addr_sender: Sender<String>,
    ) -> (PhoneIroh, Sender<bool>) {
        let (mute_sender, mute_receiver) = channel();

        let iroh = PhoneIroh {
            endpoint: None,
            active_connection: None,
            mute_receiver,
            peer_addr_receiver,
            our_addr_sender,
            muted: true,
            mic_buffer: Vec::new(),
        };

        (iroh, mute_sender)
    }

    pub async fn run(&mut self) {
        // Initialize endpoint
        if let Err(e) = self.init_endpoint().await {
            eprintln!("Failed to initialize iroh endpoint: {}", e);
            return;
        }

        let audio_system = AudioSystemMarshaller::create();

        // Create Opus encoder/decoder
        let Ok(encoder) = Encoder::new(OPUS_SAMPLE_RATE, OPUS_CHANNELS, Application::Voip) else {
            eprintln!("Failed to create Opus encoder");
            return;
        };

        let Ok(mut decoder) = Decoder::new(OPUS_SAMPLE_RATE, OPUS_CHANNELS) else {
            eprintln!("Failed to create Opus decoder");
            return;
        };

        // Track pending peer address for connection attempts
        let mut pending_peer: Option<String> = None;

        // Main loop
        loop {
            // Poll sync channels (mute + peer address)
            while let Ok(mute) = self.mute_receiver.try_recv() {
                self.muted = mute;
                audio_system.set_recording(!mute && self.active_connection.is_some());
                if mute {
                    self.mic_buffer.clear();
                }
                println!("Mute state changed: {}", mute);
            }

            while let Ok(peer_addr_str) = self.peer_addr_receiver.try_recv() {
                println!("Received peer address: {}...", &peer_addr_str[..16.min(peer_addr_str.len())]);
                // Close any existing connection so we can connect to the new peer
                // (prevents stale connections from blocking new ones)
                if let Some(conn) = self.active_connection.take() {
                    conn.close(0u32.into(), b"new peer");
                    audio_system.set_recording(false);
                    println!("Closed existing connection for new peer");
                }
                pending_peer = Some(peer_addr_str);
            }

            // Check connection health
            if let Some(conn) = &self.active_connection {
                if conn.close_reason().is_some() {
                    println!("Connection closed");
                    self.active_connection = None;
                    audio_system.set_recording(false);
                }
            }

            if self.active_connection.is_some() {
                // Connected: send/receive audio
                let conn = self.active_connection.as_ref().unwrap();

                // Drain all available mic samples into the buffer
                while let Ok(samples) = audio_system.try_receive_from_mic() {
                    self.mic_buffer.extend_from_slice(&samples);
                }
                // Send complete Opus frames
                while self.mic_buffer.len() >= OPUS_FRAME_SIZE {
                    let frame: Vec<f32> = self.mic_buffer.drain(..OPUS_FRAME_SIZE).collect();
                    if let Err(e) = self.send_audio(&encoder, conn, &frame) {
                        eprintln!("Failed to send audio: {}", e);
                    }
                }

                // Use select to receive datagrams without blocking everything
                tokio::select! {
                    datagram = conn.read_datagram() => {
                        if let Ok(datagram) = datagram {
                            if let Ok(samples) = self.decode_audio(&mut decoder, &datagram) {
                                audio_system.send_to_speaker(samples);
                            }
                        }
                    }
                    _ = tokio::time::sleep(tokio::time::Duration::from_millis(5)) => {}
                }
            } else if let Some(endpoint) = &self.endpoint {
                if let Some(ref peer_addr_str) = pending_peer {
                    // Have a peer address: try both connect AND accept simultaneously
                    // (both phones get each other's address at the same time, so we
                    // must accept while also trying to connect to avoid deadlock)
                    if let Ok(node_id) = peer_addr_str.parse::<iroh::EndpointId>() {
                        tokio::select! {
                            result = endpoint.connect(node_id, PHONEBELL_ALPN) => {
                                match result {
                                    Ok(conn) => {
                                        println!("Connected to peer: {}", conn.remote_id().fmt_short());
                                        self.active_connection = Some(conn);
                                        audio_system.set_recording(!self.muted);
                                        pending_peer = None;
                                    }
                                    Err(e) => {
                                        eprintln!("Failed to connect to peer: {}", e);
                                        pending_peer = None;
                                    }
                                }
                            }
                            incoming = endpoint.accept() => {
                                if let Some(incoming) = incoming {
                                    match incoming.await {
                                        Ok(conn) => {
                                            println!("Accepted connection from: {}", conn.remote_id().fmt_short());
                                            self.active_connection = Some(conn);
                                            audio_system.set_recording(!self.muted);
                                            pending_peer = None;
                                        }
                                        Err(e) => {
                                            eprintln!("Failed to accept connection: {}", e);
                                        }
                                    }
                                }
                            }
                            _ = tokio::time::sleep(tokio::time::Duration::from_secs(10)) => {
                                eprintln!("Connection attempt timed out, will retry...");
                            }
                        }
                    } else {
                        eprintln!("Invalid peer node ID: {}", peer_addr_str);
                        pending_peer = None;
                    }
                } else {
                    // No pending peer: just wait for incoming connections
                    tokio::select! {
                        incoming = endpoint.accept() => {
                            if let Some(incoming) = incoming {
                                match incoming.await {
                                    Ok(conn) => {
                                        println!(
                                            "Accepted connection from: {}",
                                            conn.remote_id().fmt_short()
                                        );
                                        self.active_connection = Some(conn);
                                        audio_system.set_recording(!self.muted);
                                    }
                                    Err(e) => {
                                        eprintln!("Failed to accept connection: {}", e);
                                    }
                                }
                            }
                        }
                        // Wake up periodically to check sync channels for peer address / mute
                        _ = tokio::time::sleep(tokio::time::Duration::from_millis(50)) => {}
                    }
                }
            } else {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        }
    }

    async fn init_endpoint(&mut self) -> Result<()> {
        let endpoint = Endpoint::builder()
            .alpns(vec![PHONEBELL_ALPN.to_vec()])
            .bind()
            .await?;

        // Get our node ID and send it to the socket for relay to peer
        // We send the full hex string of the node ID for cross-platform compatibility
        let node_id = endpoint.id();
        let node_id_str = node_id.to_string();
        let _ = self.our_addr_sender.send(node_id_str.clone());

        println!(
            "Iroh endpoint initialized with ID: {}",
            node_id_str
        );

        self.endpoint = Some(endpoint);
        Ok(())
    }

    fn send_audio(
        &self,
        encoder: &Encoder,
        conn: &Connection,
        samples: &[f32],
    ) -> Result<()> {
        // Opus needs fixed frame sizes, so we may need to pad or chunk
        if samples.len() < OPUS_FRAME_SIZE {
            return Ok(()); // Not enough samples yet
        }

        // Encode samples to Opus
        let mut output = vec![0u8; 1024]; // Max Opus frame size
        let encoded_len = encoder.encode_float(&samples[..OPUS_FRAME_SIZE], &mut output)?;
        output.truncate(encoded_len);

        // Send as datagram
        conn.send_datagram(output.into())?;

        Ok(())
    }

    fn decode_audio(&self, decoder: &mut Decoder, datagram: &[u8]) -> Result<Vec<f32>> {
        let mut output = vec![0f32; OPUS_FRAME_SIZE];

        // Create Packet and MutSignals wrappers for audiopus
        let packet = Packet::try_from(datagram)?;
        let signals = MutSignals::try_from(&mut output[..])?;

        let decoded_len = decoder.decode_float(Some(packet), signals, false)?;
        output.truncate(decoded_len);
        Ok(output)
    }
}

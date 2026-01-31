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

        // Main loop
        loop {
            // Check for mute state changes
            if let Ok(mute) = self.mute_receiver.try_recv() {
                self.muted = mute;
                println!("Mute state changed: {}", mute);
            }

            // Check for peer address (initiates connection)
            if let Ok(peer_addr_str) = self.peer_addr_receiver.try_recv() {
                println!("Received peer address, attempting to connect...");
                if let Err(e) = self.connect_to_peer(&peer_addr_str).await {
                    eprintln!("Failed to connect to peer: {}", e);
                }
            }

            // Accept incoming connections
            if self.active_connection.is_none() {
                if let Some(endpoint) = &self.endpoint {
                    if let Some(incoming) = endpoint.accept().await {
                        match incoming.await {
                            Ok(conn) => {
                                println!(
                                    "Accepted connection from: {}",
                                    conn.remote_id().fmt_short()
                                );
                                self.active_connection = Some(conn);
                            }
                            Err(e) => {
                                eprintln!("Failed to accept connection: {}", e);
                            }
                        }
                    }
                }
            }

            // Handle active connection audio
            if let Some(conn) = &self.active_connection {
                // Check if connection is still alive
                if conn.close_reason().is_some() {
                    println!("Connection closed");
                    self.active_connection = None;
                    continue;
                }

                // Send audio if not muted
                if !self.muted {
                    if let Ok(samples) = audio_system.try_receive_from_mic() {
                        if let Err(e) = self.send_audio(&encoder, conn, &samples) {
                            eprintln!("Failed to send audio: {}", e);
                        }
                    }
                }

                // Receive audio
                if let Ok(datagram) = conn.read_datagram().await {
                    if let Ok(samples) = self.decode_audio(&mut decoder, &datagram) {
                        audio_system.send_to_speaker(samples);
                    }
                }
            }

            // Small yield to prevent busy-looping
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
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

    async fn connect_to_peer(&mut self, peer_node_id_str: &str) -> Result<()> {
        // Parse the node ID from hex string
        let node_id: iroh::EndpointId = peer_node_id_str.parse()?;

        if let Some(endpoint) = &self.endpoint {
            let conn = endpoint.connect(node_id, PHONEBELL_ALPN).await?;
            println!(
                "Connected to peer: {}",
                conn.remote_id().fmt_short()
            );
            self.active_connection = Some(conn);
        }

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

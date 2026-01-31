use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use uuid::Uuid;

use crate::PhoneSide;

use super::{PhoneIncomingMessage, PhoneOutgoingMessage};

/// Signaling messages for iroh peer discovery (relayed via signaling WebSocket)
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum SignalingMessage {
    Join { from: String },
    JoinAck { from: String },
    IrohNodeId { node_id: String, from: String, to: String },
    Leave { from: String },
}

pub struct PhoneSocket {
    phone_side: PhoneSide,
    outgoing_receiver: Receiver<PhoneOutgoingMessage>,
    incoming_sender: Sender<PhoneIncomingMessage>,
}

pub struct SignalingSocket {
    client_id: String,
    iroh_addr_receiver: Receiver<String>,
    peer_addr_sender: Sender<String>,
}

impl PhoneSocket {
    pub fn create(
        phone_side: PhoneSide,
    ) -> (
        PhoneSocket,
        SignalingSocket,
        Sender<PhoneOutgoingMessage>,
        Receiver<PhoneIncomingMessage>,
        Sender<String>,
        Receiver<String>,
    ) {
        let (outgoing_sender, outgoing_receiver) = channel();
        let (incoming_sender, incoming_receiver) = channel();
        let (iroh_addr_sender, iroh_addr_receiver) = channel();
        let (peer_addr_sender, peer_addr_receiver) = channel();

        let client_id = Uuid::new_v4().to_string();

        let phone_socket = PhoneSocket {
            phone_side,
            outgoing_receiver,
            incoming_sender,
        };

        let signaling_socket = SignalingSocket {
            client_id,
            iroh_addr_receiver,
            peer_addr_sender,
        };

        (
            phone_socket,
            signaling_socket,
            outgoing_sender,
            incoming_receiver,
            iroh_addr_sender,
            peer_addr_receiver,
        )
    }

    pub async fn run(&mut self) {
        loop {
            if let Err(e) = self.connect_and_run().await {
                eprintln!("Phone control WebSocket error: {}, reconnecting...", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        }
    }

    async fn connect_and_run(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let url = format!(
            "wss://api.purduehackers.com/phonebell/{}",
            match self.phone_side {
                PhoneSide::Inside => "inside",
                PhoneSide::Outside => "outside",
            }
        );

        let (ws_stream, _) = connect_async(&url).await?;
        let (mut write, mut read) = ws_stream.split();

        // Send API key for authentication
        let api_key = std::env::var("PHONE_API_KEY")?;
        write.send(Message::Text(api_key.into())).await?;

        println!("Connected to phone control WebSocket: {}", url);

        loop {
            tokio::select! {
                // Handle incoming WebSocket messages
                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Text(data))) => {
                            if let Ok(message) = serde_json::from_str::<PhoneIncomingMessage>(&data) {
                                let _ = self.incoming_sender.send(message);
                            }
                        }
                        Some(Ok(Message::Ping(data))) => {
                            write.send(Message::Pong(data)).await?;
                        }
                        Some(Ok(Message::Close(_))) | None => {
                            println!("Phone control WebSocket closed");
                            return Ok(());
                        }
                        Some(Err(e)) => {
                            return Err(e.into());
                        }
                        _ => {}
                    }
                }

                // Check for outgoing messages (using a small timeout to poll)
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(10)) => {
                    // Send any pending outgoing messages
                    loop {
                        match self.outgoing_receiver.try_recv() {
                            Ok(message) => {
                                if let Ok(msg_str) = serde_json::to_string(&message) {
                                    write.send(Message::Text(msg_str.into())).await?;
                                }
                            }
                            Err(TryRecvError::Empty) => break,
                            Err(TryRecvError::Disconnected) => return Ok(()),
                        }
                    }
                }
            }
        }
    }
}

impl SignalingSocket {
    pub async fn run(&mut self) {
        loop {
            if let Err(e) = self.connect_and_run().await {
                eprintln!("Signaling WebSocket error: {}, reconnecting...", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        }
    }

    async fn connect_and_run(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let url = "wss://api.purduehackers.com/phonebell/signaling";

        let (ws_stream, _) = connect_async(url).await?;
        let (mut write, mut read) = ws_stream.split();

        println!("Connected to signaling WebSocket: {}", url);

        // Wait for server's ping and respond with pong (server requires this handshake)
        if let Some(Ok(Message::Ping(data))) = read.next().await {
            write.send(Message::Pong(data)).await?;
        }

        // Announce ourselves
        let join_msg = SignalingMessage::Join { from: self.client_id.clone() };
        if let Ok(msg_str) = serde_json::to_string(&join_msg) {
            write.send(Message::Text(msg_str.into())).await?;
        }

        // Track our current iroh node ID (updated when we receive it from iroh)
        let mut our_iroh_node_id: Option<String> = None;

        loop {
            tokio::select! {
                // Handle incoming signaling messages
                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Text(data))) => {
                            if let Ok(message) = serde_json::from_str::<SignalingMessage>(&data) {
                                // Ignore our own messages
                                let from = match &message {
                                    SignalingMessage::Join { from } => from,
                                    SignalingMessage::JoinAck { from } => from,
                                    SignalingMessage::IrohNodeId { from, .. } => from,
                                    SignalingMessage::Leave { from } => from,
                                };
                                if from == &self.client_id {
                                    continue;
                                }

                                match message {
                                    SignalingMessage::Join { from } => {
                                        println!("Peer joined: {}...", &from[..8.min(from.len())]);
                                        // Send JoinAck
                                        let ack = SignalingMessage::JoinAck { from: self.client_id.clone() };
                                        if let Ok(msg_str) = serde_json::to_string(&ack) {
                                            write.send(Message::Text(msg_str.into())).await?;
                                        }
                                        // Send our iroh node ID if we have it
                                        if let Some(ref node_id) = our_iroh_node_id {
                                            let id_msg = SignalingMessage::IrohNodeId {
                                                node_id: node_id.clone(),
                                                from: self.client_id.clone(),
                                                to: from,
                                            };
                                            if let Ok(msg_str) = serde_json::to_string(&id_msg) {
                                                write.send(Message::Text(msg_str.into())).await?;
                                            }
                                        }
                                    }
                                    SignalingMessage::JoinAck { from } => {
                                        println!("Peer acknowledged: {}...", &from[..8.min(from.len())]);
                                        // Send our iroh node ID if we have it
                                        if let Some(ref node_id) = our_iroh_node_id {
                                            let id_msg = SignalingMessage::IrohNodeId {
                                                node_id: node_id.clone(),
                                                from: self.client_id.clone(),
                                                to: from,
                                            };
                                            if let Ok(msg_str) = serde_json::to_string(&id_msg) {
                                                write.send(Message::Text(msg_str.into())).await?;
                                            }
                                        }
                                    }
                                    SignalingMessage::IrohNodeId { node_id, from, to } => {
                                        // Only process if it's for us
                                        if to == self.client_id {
                                            println!("Received iroh node ID from {}...", &from[..8.min(from.len())]);
                                            // Send to iroh module to initiate connection
                                            let _ = self.peer_addr_sender.send(node_id);
                                        }
                                    }
                                    SignalingMessage::Leave { from } => {
                                        println!("Peer left: {}...", &from[..8.min(from.len())]);
                                    }
                                }
                            }
                        }
                        Some(Ok(Message::Ping(data))) => {
                            write.send(Message::Pong(data)).await?;
                        }
                        Some(Ok(Message::Close(_))) | None => {
                            println!("Signaling WebSocket closed");
                            return Ok(());
                        }
                        Some(Err(e)) => {
                            return Err(e.into());
                        }
                        _ => {}
                    }
                }

                // Check for our iroh node ID to broadcast
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(10)) => {
                    loop {
                        match self.iroh_addr_receiver.try_recv() {
                            Ok(addr) => {
                                println!("Got our iroh node ID: {}...", &addr[..16.min(addr.len())]);
                                our_iroh_node_id = Some(addr);
                                // Note: We'll send this to peers when they Join/JoinAck
                            }
                            Err(TryRecvError::Empty) => break,
                            Err(TryRecvError::Disconnected) => return Ok(()),
                        }
                    }
                }
            }
        }
    }
}

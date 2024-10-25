use std::sync::mpsc::{channel, Receiver, Sender};

use websocket::{
    client::sync::Client,
    stream::sync::{TcpStream, TlsStream},
    ws::Sender as SenderTrait,
    ClientBuilder, Message, OwnedMessage,
};

use crate::PhoneSide;

use super::{PhoneIncomingMessage, PhoneOutgoingMessage};

pub struct PhoneSocket {
    websocket_client: Option<Client<TlsStream<TcpStream>>>,
    phone_side: PhoneSide,
    outgoing_receiver: Receiver<PhoneOutgoingMessage>,
    incoming_sender: Sender<PhoneIncomingMessage>,
}

impl PhoneSocket {
    pub fn create(
        phone_side: PhoneSide,
    ) -> (
        PhoneSocket,
        Sender<PhoneOutgoingMessage>,
        Receiver<PhoneIncomingMessage>,
    ) {
        let (outgoing_sender, outgoing_receiver) = channel();
        let (incoming_sender, incoming_receiver) = channel();

        let mut socket = PhoneSocket {
            websocket_client: None,
            phone_side,
            outgoing_receiver,
            incoming_sender,
        };

        socket.connect();

        (socket, outgoing_sender, incoming_receiver)
    }

    fn connect(&mut self) {
        if self.websocket_client.is_some() {
            return;
        }

        let Ok(mut websocket_client_builder) = ClientBuilder::new(&format!(
            "wss://api.purduehackers.com/phonebell/{}",
            match self.phone_side {
                PhoneSide::Inside => "inside",
                PhoneSide::Outside => "outside",
            }
        )) else {
            return;
        };

        let Ok(mut websocket_client) = websocket_client_builder.connect_secure(Option::None) else {
            return;
        };

        let Ok(_) =
            websocket_client.send_message(&Message::text(std::env::var("PHONE_API_KEY").unwrap()))
        else {
            return;
        };

        self.websocket_client = Some(websocket_client);
    }

    pub fn run(&mut self) {
        loop {
            if self.websocket_client.is_none() {
                self.connect();
            }

            if let Some(websocket_client) = &mut self.websocket_client {
                let mut should_shutdown = false;

                let mut sender = websocket::sender::Sender::new(true);
                let mut buf = Vec::<u8>::new();

                'message_iterate: for message in (*websocket_client).incoming_messages().flatten() {
                    match message {
                        OwnedMessage::Text(data) => {
                            let Ok(message): Result<PhoneIncomingMessage, serde_json::Error> =
                                serde_json::from_str(&data)
                            else {
                                continue;
                            };

                            let _ = self.incoming_sender.send(message);
                        }
                        OwnedMessage::Binary(_) => {}
                        OwnedMessage::Close(_) => {
                            let _ = websocket_client.shutdown();
                            should_shutdown = true;

                            break 'message_iterate;
                        }
                        OwnedMessage::Ping(data) => {
                            let _ = sender.send_message(&mut buf, &Message::pong(data));
                        }
                        OwnedMessage::Pong(_) => {}
                    }
                }

                if should_shutdown {
                    self.websocket_client = None;
                } else {
                    for message in self.outgoing_receiver.iter() {
                        let Ok(message_string) = serde_json::to_string(&message) else {
                            continue;
                        };

                        let _ = sender.send_message(&mut buf, &Message::text(message_string));
                    }

                    let _ = (*websocket_client).writer_mut().write_all(&buf);
                }
            }
        }
    }
}

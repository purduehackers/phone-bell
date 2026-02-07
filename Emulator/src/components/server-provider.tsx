"use client";

import {
	createContext,
	FC,
	PropsWithChildren,
	useCallback,
	useContext,
	useEffect,
	useRef,
	useState,
} from "react";
import ReconnectingWebSocket from "reconnecting-websocket";

export enum ConnectionState {
	Connecting,
	Connected,
	Error,
}

export enum PhoneType {
	Outside,
	Inside,
}

interface PhoneBellContextInterface {
	ringing: boolean;
	hooked: boolean;
	dialedNumber: string;

	connectionState: ConnectionState;

	setHookedState: (status: boolean) => void;
	setPhoneType: (type: PhoneType) => void;
	dialNumber: (number: string) => void;
}

const PhoneBellContext = createContext<PhoneBellContextInterface | undefined>(
	undefined,
);

export const usePhoneBellContext = () => {};

export const KNOWN_NUMBERS: string[] = [
	"0", // Operator
	"349", // "Fiz"
	"4225", // "Hack"
	"34643664", // "Dingdong",
	"8675309", // the funny
	"47932786463439686262438634258447455587853896846", // "I swear to god if you manage to dial this ill just let you in"
];

type PhoneIncomingMessage =
	| {
			type: "Dial";
			number: String;
	  }
	| {
			type: "Hook";
			state: boolean;
	  }
	| {
			type: "IrohAddr";
			addr: string;
	  };

type PhoneOutgoingMessage =
	| {
			type: "Ring";
			state: boolean;
	  }
	| { type: "ClearDial" }
	| {
			type: "PeerIrohAddr";
			addr: string;
	  };

type WebRTCSignalingMessage =
	| {
			type: "Join";
			from: string;
	  }
	| {
			type: "JoinAck";
			from: string;
	  }
	| {
			type: "ICEOffer";
			offer: RTCSessionDescriptionInit;
			from: string;
			to: string;
	  }
	| {
			type: "ICEAnswer";
			answer: RTCSessionDescriptionInit;
			from: string;
			to: string;
	  }
	| {
			type: "ICECandidate";
			candidate: RTCIceCandidate;
			from: string;
			to: string;
	  }
	| {
			type: "Leave";
			from: string;
	  };

const WEBRTC_PEER_CONNECTION_CONFIGURATION: RTCConfiguration = {
	iceServers: [{ urls: "stun:stun.l.google.com:19302" }],
};

export const PhoneBellProvider: FC<PropsWithChildren<{}>> = ({ children }) => {
	// WebRTC Audio

	const webRTCAudio = useRef<WebRTCAudioClient>();

	useEffect(() => {
		if (!webRTCAudio.current) {
			webRTCAudio.current = new WebRTCAudioClient();
			webRTCAudio.current.setMute(true);
		}
	}, []);

	// State Communications
	const [ringing, setRinging] = useState<boolean>(false);
	const [hooked, setHooked] = useState<boolean>(true);
	const [dialedNumber, setDialedNumber] = useState<string>("");
	const [enableDialing, setEnableDialing] = useState<boolean>(true);
	const [inCall, setInCall] = useState<boolean>(true);

	const [phoneType, setPhoneTypeInternal] = useState<PhoneType>(
		PhoneType.Outside,
	);

	const [connectionState, setConnectionState] = useState<ConnectionState>(
		ConnectionState.Connecting,
	);

	const phoneStateSocket = useRef<ReconnectingWebSocket | null>(null);

	const setPhoneType = (type: PhoneType) => {
		phoneStateSocket.current?.close();
		phoneStateSocket.current = null;

		setPhoneTypeInternal(type);
		setConnectionState(ConnectionState.Connecting);
	};

	useEffect(() => {
		console.log(
			`Creating WebSocket Connection of type: ${
				["Outside", "Inside"][phoneType]
			}`,
		);

		phoneStateSocket.current = new ReconnectingWebSocket(
			() =>
				`wss://api.purduehackers.com/phonebell/${
					["outside", "inside"][phoneType]
				}`,
		);

		phoneStateSocket.current.onopen = () => {
			phoneStateSocket.current?.send(process.env.PHONE_API_KEY ?? "");
			setConnectionState(ConnectionState.Connected);
		};
		phoneStateSocket.current.onclose = () => {
			setConnectionState(ConnectionState.Connecting);
		};
		phoneStateSocket.current.onerror = () => {
			setConnectionState(ConnectionState.Error);
		};
	}, [phoneType]);

	const phoneStateSocketTransmit = (message: PhoneIncomingMessage) => {
		console.log(`Phone State Socket Tx`);
		console.log(message);

		if (phoneStateSocket.current)
			phoneStateSocket.current.send(JSON.stringify(message));
	};
	const phoneStateSocketReceive = (message: PhoneOutgoingMessage) => {
		console.log(`Phone State Socket Rx`);
		console.log(message);

		switch (message.type) {
			case "Ring":
				setRinging(message.state);
				break;
			case "ClearDial":
				setDialedNumber("");
				setEnableDialing(true);
				break;
			case "PeerIrohAddr":
				// Iroh addresses from Rust phones - browser can't connect to iroh directly
				// Would need a WebRTC-to-iroh bridge on the server for audio interop
				console.log(
					`Received iroh peer address (browser cannot connect directly): ${message.addr}`,
				);
				break;
		}
	};

	useEffect(() => {
		if (!phoneStateSocket.current) return;

		phoneStateSocket.current.onmessage = (e) => {
			phoneStateSocketReceive(JSON.parse(e.data));
		};
	}, [connectionState]);

	const setHookedState = (state: boolean) => {
		webRTCAudio.current?.setMute(state);

		if (hooked != state) {
			if (state) {
				if (inCall) {
					setInCall(false);

					setEnableDialing(true);
					setDialedNumber("");
				}
			} else if (inCall) {
				setRinging(false);
			}
		}

		if (phoneStateSocket.current)
			phoneStateSocketTransmit({
				type: "Hook",
				state,
			});

		setHooked(state);
	};

	const dialNumber = (number: string) => {
		if (!enableDialing) return;

		let correctedNumber = dialedNumber + number;

		if (correctedNumber) {
			let contains = false;

			for (let number of KNOWN_NUMBERS) {
				if (number == correctedNumber) {
					contains = true;
				}
			}

			if (!contains) {
				for (let number of KNOWN_NUMBERS) {
					if (number.startsWith(correctedNumber)) {
						contains = true;
					}
				}

				if (!contains) {
					correctedNumber = "0";
				}

				contains = !contains;
			}

			if (contains) {
				setEnableDialing(false);

				if (hooked) {
					setRinging(true);
				}

				setInCall(true);

				if (phoneStateSocket.current)
					phoneStateSocketTransmit({
						type: "Dial",
						number: correctedNumber,
					});
			}
		}

		setDialedNumber(correctedNumber);
	};

	return (
		<PhoneBellContext.Provider
			value={{
				ringing,
				hooked,
				dialedNumber,

				connectionState,

				setHookedState,
				setPhoneType,
				dialNumber,
			}}
		>
			{children}
		</PhoneBellContext.Provider>
	);
};

export const usePhoneBell = (): [
	boolean,
	boolean,
	string,

	ConnectionState,

	(status: boolean) => void,
	(type: PhoneType) => void,
	(number: string) => void,
] => {
	const context = useContext(PhoneBellContext);

	if (context === undefined) {
		throw new Error("usePhoneBell must be used within a PhoneBellProvider");
	}

	return [
		context.ringing,
		context.hooked,
		context.dialedNumber,

		context.connectionState,

		context.setHookedState,
		context.setPhoneType,
		context.dialNumber,
	];
};

const uuidv4 = (): string => {
	return "10000000-1000-4000-8000-100000000000".replace(/[018]/g, (c) =>
		(
			+c ^
			(crypto.getRandomValues(new Uint8Array(1))[0] & (15 >> (+c / 4)))
		).toString(16),
	);
};

const CLIENT_UUID = uuidv4();

export class WebRTCAudioClient {
	stream?: MediaStream;
	peerConnections: { [key: string]: RTCPeerConnection } = {};
	audioStreams: { [key: string]: HTMLAudioElement } = {};
	webRTCSignalingSocket: ReconnectingWebSocket;

	muted: boolean = true;

	constructor() {
		console.log(
			`Creating WebRTC Signaling Server Connection, ClientUUID: ${CLIENT_UUID}`,
		);

		this.webRTCSignalingSocket = new ReconnectingWebSocket(
			"wss://api.purduehackers.com/phonebell/signaling",
		);

		this.webRTCSignalingSocket.onopen = () => {
			this.webRTCSignalingSocketTransmit({
				type: "Join",
				from: CLIENT_UUID,
			});
		};
		this.webRTCSignalingSocket.onmessage = (e) => {
			this.webRTCSignalingSocketReceive(JSON.parse(e.data));
		};
		this.webRTCSignalingSocket.onclose = () => {};
		this.webRTCSignalingSocket.onerror = () => {};

		navigator.mediaDevices
			.getUserMedia({ video: false, audio: true })
			.then((currentStream) => {
				for (let track of currentStream.getAudioTracks()) {
					for (let peerConnection of Object.values(
						this.peerConnections,
					)) {
						peerConnection.addTrack(track, currentStream);
					}

					track.enabled = !this.muted;
				}

				this.stream = currentStream;
			});
	}

	public setMute = (state: boolean) => {
		this.muted = state;

		if (this.stream) {
			for (let track of this.stream.getAudioTracks()) {
				track.enabled = !state;
			}
		}

		for (let audioStream of Object.values(this.audioStreams)) {
			audioStream.muted = state;
		}
	};

	registerPeerConnection = (
		peerConnection: RTCPeerConnection,
		target: string,
	) => {
		this.peerConnections[target] = peerConnection;

		if (this.stream) {
			for (let track of this.stream.getAudioTracks()) {
				peerConnection.addTrack(track, this.stream);
			}
		}

		peerConnection.addEventListener("connectionstatechange", (event) => {
			console.log(
				`ICEOffer Connection State Changed To: ${peerConnection.connectionState}`,
			);

			if (
				peerConnection.connectionState == "disconnected" ||
				peerConnection.connectionState == "failed"
			) {
				if (this.peerConnections[target])
					delete this.peerConnections[target];

				if (this.audioStreams[target]) delete this.audioStreams[target];
			}
		});

		peerConnection.addEventListener("track", async (event) => {
			const [remoteStream] = event.streams;

			let newAudioElement = document.createElement("audio");

			newAudioElement.srcObject = remoteStream;
			newAudioElement.autoplay = true;

			this.audioStreams[target] = newAudioElement;
		});
	};

	webRTCSignalingSocketTransmit = (message: WebRTCSignalingMessage) => {
		console.log(
			`%c WebRTC Signaling Socket Transmit: ${message.type}`,
			"color: #00aaff",
		);
		console.log(message);

		this.webRTCSignalingSocket.send(JSON.stringify(message));
	};
	webRTCSignalingSocketReceive = (message: WebRTCSignalingMessage) => {
		console.log(
			`%c WebRTC Signaling Socket Receive: ${message.type}`,
			"color: #ff00aa",
		);
		console.log(message);

		switch (message.type) {
			case "Join":
				if (message.from != CLIENT_UUID) {
					this.webRTCSignalingSocketTransmit({
						type: "JoinAck",
						from: CLIENT_UUID,
					});
				}
				break;
			case "JoinAck":
				if (
					message.from != CLIENT_UUID &&
					!this.peerConnections[message.from]
				) {
					console.log(`New JoinAck WebRTC Client: ${message.from}`);

					const newPeerConnection = new RTCPeerConnection(
						WEBRTC_PEER_CONNECTION_CONFIGURATION,
					);

					this.registerPeerConnection(
						newPeerConnection,
						message.from,
					);

					(async () => {
						const offer = await newPeerConnection.createOffer({
							offerToReceiveAudio: true,
							offerToReceiveVideo: false,
						});

						await newPeerConnection.setLocalDescription(offer);

						this.webRTCSignalingSocketTransmit({
							type: "ICEOffer",
							offer,
							from: CLIENT_UUID,
							to: message.from,
						});
					})();
				}

				break;
			case "ICEOffer":
				if (
					message.from != CLIENT_UUID &&
					message.to == CLIENT_UUID &&
					!this.peerConnections[message.from]
				) {
					console.log(`New ICEOffer WebRTC Client: ${message.from}`);

					const newPeerConnection = new RTCPeerConnection(
						WEBRTC_PEER_CONNECTION_CONFIGURATION,
					);

					this.registerPeerConnection(
						newPeerConnection,
						message.from,
					);

					(async () => {
						await newPeerConnection.setRemoteDescription(
							new RTCSessionDescription(message.offer),
						);

						const answer = await newPeerConnection.createAnswer({
							offerToReceiveAudio: true,
							offerToReceiveVideo: false,
						});
						await newPeerConnection.setLocalDescription(answer);

						this.webRTCSignalingSocketTransmit({
							type: "ICEAnswer",
							answer,
							from: CLIENT_UUID,
							to: message.from,
						});

						newPeerConnection.addEventListener(
							"icecandidate",
							(e) => {
								console.log("candidate info", e.candidate);

								if (e.candidate) {
									this.webRTCSignalingSocketTransmit({
										type: "ICECandidate",
										candidate: e.candidate,
										from: CLIENT_UUID,
										to: message.from,
									});
								}
							},
						);
					})();
				}

				break;
			case "ICEAnswer":
				if (
					message.from != CLIENT_UUID &&
					message.to == CLIENT_UUID &&
					this.peerConnections[message.from]
				) {
					console.log(`Valid ICEAnswer from: ${message.from}`);
					console.log(this.peerConnections);

					(async () => {
						await this.peerConnections[
							message.from
						].setRemoteDescription(
							new RTCSessionDescription(message.answer),
						);

						this.peerConnections[message.from].addEventListener(
							"icecandidate",
							(e) => {
								console.log("candidate info", e.candidate);

								if (e.candidate) {
									this.webRTCSignalingSocketTransmit({
										type: "ICECandidate",
										candidate: e.candidate,
										from: CLIENT_UUID,
										to: message.from,
									});
								}
							},
						);
					})();
				}
				break;
			case "ICECandidate":
				if (
					message.from != CLIENT_UUID &&
					message.to == CLIENT_UUID &&
					this.peerConnections[message.from]
				) {
					console.log(`Valid ICECandidate from: ${message.from}`);

					(async () => {
						if (message.candidate)
							await this.peerConnections[
								message.from
							].addIceCandidate(message.candidate);
					})();
				}
				break;
		}
	};
}

// const webRTCAudio = new WebRTCAudioClient();

import "dotenv/config";
import { Endpoint, Connection } from "@rayhanadev/iroh";
import WebSocket from "ws";
import * as readline from "readline";
import { randomUUID } from "crypto";

const PHONEBELL_ALPN = "phonebell/voip/1";

// Known phone numbers (same as Rust backend)
const KNOWN_NUMBERS: string[] = [
  "0", // Operator
  "7", // Silent
  "349", // "Fiz"
  "4225", // "Hack"
  "34643664", // "Dingdong"
  "8675309", // Jenny
  "47932786463439686262438634258447455587853896846", // Easter egg
];

type PhoneType = "inside" | "outside";

interface PhoneState {
  hooked: boolean;
  ringing: boolean;
  dialedNumber: string;
  inCall: boolean;
  enableDialing: boolean;
}

// Message types for phone control WebSocket
type PhoneOutgoingMessage =
  | { type: "Dial"; number: string }
  | { type: "Hook"; state: boolean };

type PhoneIncomingMessage =
  | { type: "Ring"; state: boolean }
  | { type: "ClearDial" };

// Message types for signaling WebSocket (relayed to all peers)
type SignalingMessage =
  | { type: "Join"; from: string }
  | { type: "JoinAck"; from: string }
  | { type: "IrohNodeId"; nodeId: string; from: string; to: string }
  | { type: "Leave"; from: string };

class PhoneBellEmulator {
  private endpoint: Endpoint | null = null;
  private phoneControlWs: WebSocket | null = null;
  private signalingWs: WebSocket | null = null;
  private phoneType: PhoneType;
  private state: PhoneState;
  private rl: readline.Interface;
  private connection: Connection | null = null;
  private isConnectingToPeer: boolean = false;
  private clientId: string;
  private peers: Map<string, string> = new Map(); // clientId -> iroh nodeId

  constructor(phoneType: PhoneType) {
    this.phoneType = phoneType;
    this.clientId = randomUUID();
    this.state = {
      hooked: true,
      ringing: false,
      dialedNumber: "",
      inCall: false,
      enableDialing: true,
    };

    this.rl = readline.createInterface({
      input: process.stdin,
      output: process.stdout,
    });
  }

  async start() {
    console.log(`\n🔔 Phone Bell Emulator (${this.phoneType})`);
    console.log(`   Client ID: ${this.clientId.slice(0, 8)}...`);
    console.log("=".repeat(40));

    // Initialize iroh endpoint
    await this.initIroh();

    // Connect to phone control WebSocket
    this.connectPhoneControlWs();

    // Connect to signaling WebSocket
    this.connectSignalingWs();

    // Start CLI interface
    this.startCLI();

    // Start accepting iroh connections (non-blocking)
    this.acceptIrohConnections();
  }

  private async initIroh() {
    console.log("📡 Initializing iroh endpoint...");

    this.endpoint = await Endpoint.createWithOptions({
      alpns: [PHONEBELL_ALPN],
    });

    await this.endpoint.online();

    const nodeId = this.endpoint.nodeId();
    console.log(`✅ Iroh ready. Node ID: ${nodeId.slice(0, 16)}...`);
  }

  // ============ Phone Control WebSocket ============

  private connectPhoneControlWs() {
    const url = `wss://api.purduehackers.com/phonebell/${this.phoneType}`;
    console.log(`🔌 Connecting to phone control: ${url}...`);

    this.phoneControlWs = new WebSocket(url);

    this.phoneControlWs.on("open", () => {
      console.log("✅ Phone control WebSocket connected");

      // Send API key
      const apiKey = process.env.PHONE_API_KEY;
      if (apiKey) {
        this.phoneControlWs?.send(apiKey);
      } else {
        console.warn("⚠️  PHONE_API_KEY not set");
      }
    });

    this.phoneControlWs.on("message", (data) => {
      const raw = data.toString();
      try {
        const message: PhoneIncomingMessage = JSON.parse(raw);
        this.handlePhoneControlMessage(message);
      } catch (e) {
        // Ignore non-JSON messages
      }
    });

    this.phoneControlWs.on("close", () => {
      console.log("\n❌ Phone control disconnected, reconnecting...");
      setTimeout(() => this.connectPhoneControlWs(), 1000);
    });

    this.phoneControlWs.on("error", (err) => {
      console.error("\nPhone control error:", err.message);
    });
  }

  private handlePhoneControlMessage(message: PhoneIncomingMessage) {
    switch (message.type) {
      case "Ring":
        this.state.ringing = message.state;
        if (message.state) {
          console.log("\n🔔 RING RING RING!");
        } else {
          console.log("\n🔕 Ringing stopped");
        }
        this.printPrompt();
        break;

      case "ClearDial":
        this.state.dialedNumber = "";
        this.state.enableDialing = true;
        this.state.inCall = false;
        console.log("\n📞 Dial cleared");
        this.printPrompt();
        break;
    }
  }

  private sendPhoneControlMessage(message: PhoneOutgoingMessage) {
    if (this.phoneControlWs?.readyState === WebSocket.OPEN) {
      this.phoneControlWs.send(JSON.stringify(message));
    }
  }

  // ============ Signaling WebSocket ============

  private connectSignalingWs() {
    const url = "wss://api.purduehackers.com/phonebell/signaling";
    console.log(`🔌 Connecting to signaling: ${url}...`);

    this.signalingWs = new WebSocket(url);

    this.signalingWs.on("open", () => {
      console.log("✅ Signaling WebSocket connected");
    });

    // Wait for server's ping before sending Join (server sends ping as handshake)
    this.signalingWs.once("ping", () => {
      this.sendSignalingMessage({ type: "Join", from: this.clientId });
    });

    this.signalingWs.on("message", (data) => {
      const raw = data.toString();
      try {
        const message: SignalingMessage = JSON.parse(raw);
        this.handleSignalingMessage(message);
      } catch (e) {
        // Ignore non-JSON messages
      }
    });

    this.signalingWs.on("close", () => {
      console.log("\n❌ Signaling disconnected, reconnecting...");
      setTimeout(() => this.connectSignalingWs(), 1000);
    });

    this.signalingWs.on("error", (err) => {
      console.error("\nSignaling error:", err.message);
    });
  }

  private handleSignalingMessage(message: SignalingMessage) {
    // Ignore our own messages
    if (message.from === this.clientId) return;

    switch (message.type) {
      case "Join":
        console.log(`\n👋 Peer joined: ${message.from.slice(0, 8)}...`);
        // Send JoinAck with our iroh node ID
        this.sendSignalingMessage({ type: "JoinAck", from: this.clientId });
        // Send our iroh node ID to the new peer
        if (this.endpoint) {
          this.sendSignalingMessage({
            type: "IrohNodeId",
            nodeId: this.endpoint.nodeId(),
            from: this.clientId,
            to: message.from,
          });
        }
        this.printPrompt();
        break;

      case "JoinAck":
        console.log(`\n👋 Peer acknowledged: ${message.from.slice(0, 8)}...`);
        // Send our iroh node ID to this peer
        if (this.endpoint) {
          this.sendSignalingMessage({
            type: "IrohNodeId",
            nodeId: this.endpoint.nodeId(),
            from: this.clientId,
            to: message.from,
          });
        }
        this.printPrompt();
        break;

      case "IrohNodeId":
        // Only process if it's for us
        if (message.to !== this.clientId) return;
        console.log(
          `\n📥 Received iroh node ID from ${message.from.slice(0, 8)}...`,
        );
        this.peers.set(message.from, message.nodeId);
        // Connect to this peer
        this.connectToPeer(message.nodeId);
        this.printPrompt();
        break;

      case "Leave":
        console.log(`\n👋 Peer left: ${message.from.slice(0, 8)}...`);
        this.peers.delete(message.from);
        this.printPrompt();
        break;
    }
  }

  private sendSignalingMessage(message: SignalingMessage) {
    if (this.signalingWs?.readyState === WebSocket.OPEN) {
      this.signalingWs.send(JSON.stringify(message));
    }
  }

  // ============ Iroh Connection ============

  private async connectToPeer(nodeId: string) {
    if (!this.endpoint || this.isConnectingToPeer) return;
    if (this.connection) {
      console.log("Already connected to a peer");
      return;
    }

    this.isConnectingToPeer = true;

    try {
      console.log("🔗 Connecting to peer via iroh...");
      this.connection = await this.endpoint.connect(nodeId, PHONEBELL_ALPN);
      console.log("\n✅ Connected to peer via iroh!");
      this.printPrompt();

      // Start receiving audio datagrams in background
      this.receiveAudio();
    } catch (e: any) {
      console.error("\n❌ Failed to connect to peer:", e.message);
      this.printPrompt();
    } finally {
      this.isConnectingToPeer = false;
    }
  }

  private async acceptIrohConnections() {
    if (!this.endpoint) return;

    // Run accept loop in background
    (async () => {
      while (this.endpoint && !this.endpoint.isClosed()) {
        // Skip if already connected
        if (this.connection) {
          await new Promise((r) => setTimeout(r, 1000));
          continue;
        }

        try {
          const conn = await this.endpoint.accept();
          if (conn && !this.connection) {
            console.log("\n📥 Incoming iroh connection accepted!");
            this.connection = conn;
            this.printPrompt();

            // Start receiving audio datagrams
            this.receiveAudio();
          } else if (conn) {
            // Already have a connection, close this one
            conn.close(0, "already connected");
          }
        } catch (e: any) {
          if (!this.endpoint?.isClosed() && !this.connection) {
            console.error("\nAccept error:", e.message);
          }
          // Don't break, keep trying if not connected
          if (!this.connection) {
            await new Promise((r) => setTimeout(r, 1000));
          }
        }
      }
    })();
  }

  private async receiveAudio() {
    if (!this.connection) return;

    try {
      while (this.connection) {
        const datagram = await this.connection.readDatagram();
        if (datagram && datagram.length > 0) {
          process.stdout.write("🎵");
        }
      }
    } catch (e) {
      console.log("\n🔇 Audio stream ended");
      this.connection = null;
      this.printPrompt();
    }
  }

  // ============ CLI Interface ============

  private startCLI() {
    console.log("\nCommands:");
    console.log("  0-9    - Dial a number");
    console.log("  h      - Toggle hook (pick up / hang up)");
    console.log("  t      - Send test audio datagram");
    console.log("  s      - Show status");
    console.log("  q      - Quit");
    console.log("");

    this.printPrompt();

    this.rl.on("line", (input) => {
      this.handleInput(input.trim().toLowerCase());
      this.printPrompt();
    });
  }

  private printPrompt() {
    const hookStatus = this.state.hooked ? "🔴 On Hook" : "🟢 Off Hook";
    const ringStatus = this.state.ringing ? " 🔔" : "";
    const irohStatus = this.connection ? " 📡" : "";
    process.stdout.write(`\n[${hookStatus}${ringStatus}${irohStatus}] > `);
  }

  private handleInput(input: string) {
    if (input >= "0" && input <= "9") {
      this.dialDigit(input);
    } else if (input === "h") {
      this.toggleHook();
    } else if (input === "t") {
      this.sendTestAudio();
    } else if (input === "s") {
      this.showStatus();
    } else if (input === "q") {
      this.quit();
    }
  }

  private dialDigit(digit: string) {
    if (!this.state.enableDialing) {
      console.log("Dialing disabled");
      return;
    }

    let newNumber = this.state.dialedNumber + digit;

    let isValid = false;
    for (const num of KNOWN_NUMBERS) {
      if (num === newNumber || num.startsWith(newNumber)) {
        isValid = true;
        break;
      }
    }

    if (!isValid) {
      newNumber = "0";
    }

    this.state.dialedNumber = newNumber;
    console.log(`Dialed: ${newNumber}`);

    if (KNOWN_NUMBERS.includes(newNumber)) {
      console.log(`📞 Calling: ${newNumber}`);
      this.state.enableDialing = false;
      this.state.inCall = true;
      this.sendPhoneControlMessage({ type: "Dial", number: newNumber });
    }
  }

  private toggleHook() {
    this.state.hooked = !this.state.hooked;

    if (this.state.hooked) {
      console.log("📞 Hung up");
      if (this.state.inCall) {
        this.state.inCall = false;
        this.state.enableDialing = true;
        this.state.dialedNumber = "";
      }
      this.state.ringing = false;
    } else {
      console.log("📞 Picked up");
      if (this.state.ringing) {
        this.state.ringing = false;
        this.state.inCall = true;  // Answering incoming call
      }
    }

    this.sendPhoneControlMessage({ type: "Hook", state: this.state.hooked });
  }

  private async sendTestAudio() {
    if (!this.connection) {
      console.log("❌ Not connected to peer");
      return;
    }

    try {
      const testData = Buffer.from("Test audio packet " + Date.now());
      await this.connection.sendDatagram(testData);
      console.log("📤 Sent test audio datagram");
    } catch (e: any) {
      console.error("Failed to send:", e.message);
    }
  }

  private showStatus() {
    console.log("\n--- Status ---");
    console.log(`Phone Type: ${this.phoneType}`);
    console.log(`Client ID: ${this.clientId.slice(0, 8)}...`);
    console.log(`Hooked: ${this.state.hooked}`);
    console.log(`Ringing: ${this.state.ringing}`);
    console.log(`Dialed Number: ${this.state.dialedNumber || "(none)"}`);
    console.log(`In Call: ${this.state.inCall}`);
    console.log(`Iroh Endpoint: ${this.endpoint ? "Ready" : "Not ready"}`);
    console.log(
      `Iroh Node ID: ${this.endpoint?.nodeId().slice(0, 16) || "N/A"}...`,
    );
    console.log(`Iroh Connected: ${this.connection ? "Yes" : "No"}`);
    console.log(`Known Peers: ${this.peers.size}`);
    for (const [clientId, nodeId] of this.peers) {
      console.log(
        `  - ${clientId.slice(0, 8)}... -> ${nodeId.slice(0, 16)}...`,
      );
    }
    console.log(
      `Phone Control WS: ${this.phoneControlWs?.readyState === WebSocket.OPEN ? "Connected" : "Disconnected"}`,
    );
    console.log(
      `Signaling WS: ${this.signalingWs?.readyState === WebSocket.OPEN ? "Connected" : "Disconnected"}`,
    );
    console.log("--------------");
  }

  private quit() {
    console.log("\nGoodbye! 👋");
    // Announce leave
    this.sendSignalingMessage({ type: "Leave", from: this.clientId });
    this.connection?.close(0, "quit");
    this.endpoint?.close();
    this.phoneControlWs?.close();
    this.signalingWs?.close();
    this.rl.close();
    process.exit(0);
  }
}

// Main
const phoneType = (process.argv[2] as PhoneType) || "outside";
if (phoneType !== "inside" && phoneType !== "outside") {
  console.error("Usage: npm start [inside|outside]");
  process.exit(1);
}

const emulator = new PhoneBellEmulator(phoneType);
emulator.start().catch(console.error);

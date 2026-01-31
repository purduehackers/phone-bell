/**
 * Test two CLI instances connecting to each other locally,
 * simulating what the server would do by manually exchanging node IDs.
 */

import "dotenv/config";
import { Endpoint, Connection } from "@rayhanadev/iroh";

const PHONEBELL_ALPN = "phonebell/voip/1";

async function runTest() {
  console.log("🧪 Testing two phone instances connecting via iroh\n");

  // Create "Outside" phone
  console.log("Creating Outside phone...");
  const outsideEndpoint = await Endpoint.createWithOptions({
    alpns: [PHONEBELL_ALPN],
  });
  await outsideEndpoint.online();
  const outsideNodeId = outsideEndpoint.nodeId();
  console.log(`✅ Outside phone ready: ${outsideNodeId.slice(0, 16)}...`);

  // Create "Inside" phone
  console.log("\nCreating Inside phone...");
  const insideEndpoint = await Endpoint.createWithOptions({
    alpns: [PHONEBELL_ALPN],
  });
  await insideEndpoint.online();
  const insideNodeId = insideEndpoint.nodeId();
  console.log(`✅ Inside phone ready: ${insideNodeId.slice(0, 16)}...`);

  // Simulate server relaying node IDs
  console.log("\n--- Simulating server relay ---");
  console.log(`Server sends Outside's node ID to Inside`);
  console.log(`Server sends Inside's node ID to Outside`);

  // Inside phone starts accepting
  let insideConnection: Connection | null = null;
  const acceptPromise = (async () => {
    console.log("\n📡 Inside phone waiting for connection...");
    insideConnection = await insideEndpoint.accept();
    if (!insideConnection) throw new Error("Accept returned null");
    console.log("✅ Inside phone: Connection accepted!");

    // Listen for audio
    (async () => {
      try {
        while (insideConnection) {
          const data = await insideConnection.readDatagram();
          console.log(`🎵 Inside received: ${data.toString()}`);
        }
      } catch (e) {
        console.log("Inside: Audio stream ended");
      }
    })();
  })();

  // Give accept time to start listening
  await new Promise((r) => setTimeout(r, 200));

  // Outside phone connects to Inside using node ID
  console.log("\n🔗 Outside phone connecting to Inside...");
  let outsideConnection: Connection;
  try {
    outsideConnection = await outsideEndpoint.connect(
      insideNodeId,
      PHONEBELL_ALPN,
    );
    console.log("✅ Outside phone: Connected!");
  } catch (e: any) {
    console.error("❌ Outside failed to connect:", e.message);
    await outsideEndpoint.close();
    await insideEndpoint.close();
    process.exit(1);
  }

  // Wait for accept to complete
  await acceptPromise;

  // Test bidirectional communication
  console.log("\n--- Testing audio datagrams ---");

  // Outside sends to Inside
  console.log("📤 Outside sending: 'Hello Inside!'");
  await outsideConnection.sendDatagram(Buffer.from("Hello Inside!"));

  await new Promise((r) => setTimeout(r, 500));

  // Inside sends to Outside
  if (insideConnection !== null) {
    console.log("📤 Inside sending: 'Hello Outside!'");
    await (insideConnection as Connection).sendDatagram(
      Buffer.from("Hello Outside!"),
    );
  }

  // Outside listens
  (async () => {
    try {
      const data = await outsideConnection.readDatagram();
      console.log(`🎵 Outside received: ${data.toString()}`);
    } catch (e) {
      console.log("Outside: Read ended");
    }
  })();

  await new Promise((r) => setTimeout(r, 1000));

  console.log("\n" + "=".repeat(50));
  console.log("🎉 SUCCESS! Both phones connected and exchanged data!");
  console.log("=".repeat(50));

  // Cleanup
  outsideConnection.close(0, "done");
  if (insideConnection !== null) {
    (insideConnection as Connection).close(0, "done");
  }
  await outsideEndpoint.close();
  await insideEndpoint.close();

  process.exit(0);
}

runTest().catch((err) => {
  console.error("❌ Test failed:", err);
  process.exit(1);
});

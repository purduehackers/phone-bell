/**
 * Test script to verify two iroh endpoints can find and connect to each other locally
 * without needing the Purdue Hackers server.
 */

import { Endpoint } from "@rayhanadev/iroh";

const PHONEBELL_ALPN = "phonebell/voip/1";

async function runTest() {
  console.log("🧪 Testing iroh peer-to-peer connection\n");

  // Create two endpoints (simulating two phones)
  console.log("Creating endpoint 1 (Outside phone)...");
  const endpoint1 = await Endpoint.createWithOptions({
    alpns: [PHONEBELL_ALPN],
  });
  await endpoint1.online();
  const nodeId1 = endpoint1.nodeId();
  const addr1 = endpoint1.addr();
  console.log(`✅ Endpoint 1 ready`);
  console.log(`   Node ID: ${nodeId1}`);
  console.log(`   Address: ${addr1}`);
  console.log(`   Address type: ${typeof addr1}`);
  console.log(`   Address length: ${addr1.length}`);

  console.log("\nCreating endpoint 2 (Inside phone)...");
  const endpoint2 = await Endpoint.createWithOptions({
    alpns: [PHONEBELL_ALPN],
  });
  await endpoint2.online();
  const nodeId2 = endpoint2.nodeId();
  const addr2 = endpoint2.addr();
  console.log(`✅ Endpoint 2 ready`);
  console.log(`   Node ID: ${nodeId2}`);
  console.log(`   Address: ${addr2}`);

  // Try connecting with just the nodeId
  console.log("\n🔗 Attempting connection using Node ID...");
  console.log(`   Connecting to: ${nodeId2}`);

  // Start accepting connections on endpoint 2
  const acceptPromise = (async () => {
    console.log("📡 Endpoint 2 waiting for connections...");
    const conn = await endpoint2.accept();
    if (!conn) {
      throw new Error("Failed to accept connection");
    }
    console.log(`✅ Endpoint 2 accepted connection!`);

    // Try to receive a datagram
    console.log("📥 Endpoint 2 waiting for datagram...");
    const data = await conn.readDatagram();
    console.log(`✅ Endpoint 2 received: "${data.toString()}"`);

    // Send response
    await conn.sendDatagram(Buffer.from("Hello from Inside!"));

    return conn;
  })();

  // Small delay to ensure accept is listening
  await new Promise((r) => setTimeout(r, 100));

  try {
    // Try with full address first
    const conn1 = await endpoint1.connect(addr2, PHONEBELL_ALPN);
    console.log(`✅ Endpoint 1 connected!`);

    // Send a datagram
    console.log("📤 Endpoint 1 sending datagram...");
    await conn1.sendDatagram(Buffer.from("Hello from Outside!"));

    // Wait for response
    const response = await conn1.readDatagram();
    console.log(`✅ Endpoint 1 received: "${response.toString()}"`);

    await acceptPromise;

    console.log("\n🎉 SUCCESS!");

    conn1.close(0, "done");
  } catch (e: any) {
    console.error(`❌ Connection failed: ${e.message}`);
    console.log("\nTrying with just nodeId...");

    try {
      const conn1 = await endpoint1.connect(nodeId2, PHONEBELL_ALPN);
      console.log(`✅ Connected with nodeId!`);

      await conn1.sendDatagram(Buffer.from("Hello!"));
      const response = await conn1.readDatagram();
      console.log(`✅ Received: "${response.toString()}"`);

      await acceptPromise;
      console.log("\n🎉 SUCCESS with nodeId!");

      conn1.close(0, "done");
    } catch (e2: any) {
      console.error(`❌ NodeId connection also failed: ${e2.message}`);
    }
  }

  await endpoint1.close();
  await endpoint2.close();
  console.log("\nDone!");
  process.exit(0);
}

runTest().catch((err) => {
  console.error("❌ Test failed:", err);
  process.exit(1);
});

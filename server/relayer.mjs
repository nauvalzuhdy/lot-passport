// Lot Passport relayer: pays gas for suppliers and inspectors, so they never
// need to buy NEAR (NEP-366 meta transactions). It also proxies dry NEAR
// Intents quotes for the browser.
//
// Run:  cd server && npm install && RELAYER_ACCOUNT=... RELAYER_KEY=ed25519:... CONTRACT_ID=... npm start
// The relayer account needs a little testnet NEAR for gas. Never commit its key.

import http from "node:http";
import { connect, keyStores, KeyPair, transactions, utils } from "near-api-js";
import { checkDelegate, checkQuote } from "./policy.mjs";

const {
  RELAYER_ACCOUNT, RELAYER_KEY, CONTRACT_ID,
  NETWORK = "testnet",
  RPC = "https://rpc.testnet.fastnear.com",
  PORT = "8787",
  ALLOW_ORIGIN = "http://localhost:5173",
  ONECLICK_JWT = "",
} = process.env;

if (!RELAYER_ACCOUNT || !RELAYER_KEY || !CONTRACT_ID) {
  console.error("Set RELAYER_ACCOUNT, RELAYER_KEY and CONTRACT_ID (see README, Gasless signing).");
  process.exit(1);
}

const keyStore = new keyStores.InMemoryKeyStore();
await keyStore.setKey(NETWORK, RELAYER_ACCOUNT, KeyPair.fromString(RELAYER_KEY));
const near = await connect({ networkId: NETWORK, nodeUrl: RPC, keyStore });
const relayer = await near.account(RELAYER_ACCOUNT);
const { actionCreators, buildDelegateAction, Signature } = transactions;

function send(res, status, body) {
  res.writeHead(status, {
    "Content-Type": "application/json",
    "Access-Control-Allow-Origin": ALLOW_ORIGIN,
    "Access-Control-Allow-Methods": "POST, GET, OPTIONS",
    "Access-Control-Allow-Headers": "Content-Type",
  });
  res.end(JSON.stringify(body));
}

async function readJson(req) {
  let raw = "";
  for await (const chunk of req) {
    raw += chunk;
    if (raw.length > 64_000) throw new Error("Body too large");
  }
  return JSON.parse(raw || "{}");
}

async function relay(body) {
  const block = await near.connection.provider.block({ finality: "final" });
  const check = checkDelegate(body, { contractId: CONTRACT_ID, currentHeight: block.header.height });
  if (!check.ok) return [400, { error: check.reason }];

  // Rebuild the exact delegate action the user signed. If anything differs,
  // the signature no longer matches and the chain rejects it.
  const delegateAction = buildDelegateAction({
    senderId: body.senderId,
    receiverId: body.receiverId,
    publicKey: utils.PublicKey.fromString(body.publicKey),
    nonce: BigInt(body.nonce),
    maxBlockHeight: BigInt(body.maxBlockHeight),
    actions: body.actions.map((a) =>
      actionCreators.functionCall(a.methodName, Buffer.from(a.argsBase64, "base64"), BigInt(a.gas), 0n),
    ),
  });
  const signature = new Signature({ keyType: 0, data: Buffer.from(body.signature, "base64") });
  const outcome = await relayer.signAndSendTransaction({
    receiverId: body.senderId,
    actions: [actionCreators.signedDelegate({ delegateAction, signature })],
  });
  return [200, { hash: outcome.transaction.hash, status: outcome.status }];
}

async function quote(body) {
  const check = checkQuote(body);
  if (!check.ok) return [400, { error: check.reason }];
  const r = await fetch("https://1click.chaindefuser.com/v0/quote", {
    method: "POST",
    headers: { "Content-Type": "application/json", ...(ONECLICK_JWT ? { Authorization: `Bearer ${ONECLICK_JWT}` } : {}) },
    body: JSON.stringify(body),
  });
  return [r.status, await r.json().catch(() => ({ error: "Bad response from 1Click" }))];
}

http
  .createServer(async (req, res) => {
    if (req.method === "OPTIONS") return send(res, 204, {});
    try {
      if (req.method === "GET" && req.url === "/health") return send(res, 200, { ok: true, relayer: RELAYER_ACCOUNT, contract: CONTRACT_ID });
      if (req.method === "POST" && req.url === "/relay") return send(res, ...(await relay(await readJson(req))));
      if (req.method === "POST" && req.url === "/quote") return send(res, ...(await quote(await readJson(req))));
      send(res, 404, { error: "Not found" });
    } catch (e) {
      send(res, 500, { error: String(e.message || e) });
    }
  })
  .listen(Number(PORT), () => console.log(`Relayer for ${CONTRACT_ID} on http://localhost:${PORT} (paying as ${RELAYER_ACCOUNT})`));

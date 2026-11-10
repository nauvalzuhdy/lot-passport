// Minimal NEAR helpers: read the contract over JSON-RPC, sign through the
// user's wallet with near-connect, and print the equivalent near-cli-rs
// command for every write, so each step can also be done by hand.
import { CONFIG } from "./config.js";
import { NearConnector } from "@hot-labs/near-connect";

export const TGAS = 1_000_000_000_000n;
export const ONE_NEAR = 10n ** 24n;

function b64(str) {
  const bytes = new TextEncoder().encode(str);
  let bin = "";
  bytes.forEach((b) => (bin += String.fromCharCode(b)));
  return btoa(bin);
}

async function rpc(method, params) {
  const res = await fetch(CONFIG.rpc, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: "lot-passport", method, params }),
  });
  if (!res.ok) throw new Error(`RPC ${res.status}: ${await res.text()}`);
  const j = await res.json();
  if (j.error) throw new Error(j.error.data || j.error.message || JSON.stringify(j.error));
  return j.result;
}

/** Calls a view method. `contract` defaults to the Lot Passport contract. */
export async function view(method, args = {}, contract = CONFIG.contractId) {
  const r = await rpc("query", {
    request_type: "call_function",
    finality: "optimistic",
    account_id: contract,
    method_name: method,
    args_base64: b64(JSON.stringify(args)),
  });
  if (r.error) throw new Error(r.error);
  const text = new TextDecoder().decode(new Uint8Array(r.result));
  return text ? JSON.parse(text) : null;
}

// ---------- wallet ----------
let connector;
let wallet = null;

export function initWallet(onChange) {
  connector = new NearConnector({ network: CONFIG.network });
  // Restore a previous connection, if the wallet kept one.
  connector
    .wallet()
    .then(async (w) => {
      wallet = w;
      onChange(await currentAccount());
    })
    .catch(() => onChange(null));
  return connector;
}

export async function connect() {
  wallet = await connector.connect();
  return currentAccount();
}

export async function disconnect() {
  try {
    await connector.disconnect();
  } finally {
    wallet = null;
  }
}

export async function currentAccount() {
  if (!wallet) return null;
  const accounts = await wallet.getAccounts();
  return accounts && accounts[0] ? accounts[0].accountId : null;
}

/**
 * Signs and sends one function call. Returns the transaction hash.
 * gasTgas: number, depositYocto: string or bigint.
 */
export async function call(receiverId, methodName, args, { gasTgas = 100, depositYocto = "0" } = {}) {
  if (!wallet) throw new Error("Connect a wallet first");
  const outcome = await wallet.signAndSendTransaction({
    receiverId,
    actions: [
      {
        type: "FunctionCall",
        params: { methodName, args, gas: (BigInt(gasTgas) * TGAS).toString(), deposit: depositYocto.toString() },
      },
    ],
  });
  const failure = outcome?.status?.Failure;
  if (failure) throw new Error("Transaction failed: " + JSON.stringify(failure));
  return outcome?.transaction?.hash || outcome?.transaction_outcome?.id || null;
}

/** The same call as a near-cli-rs command, for the README and for doing it by hand. */
export function cliCommand(signer, receiverId, methodName, args, { gasTgas = 100, depositYocto = "0" } = {}) {
  const dep = BigInt(depositYocto);
  const depText = dep === 0n ? "0 NEAR" : dep === 1n ? "1 yoctoNEAR" : `${formatUnits(dep, 24, 6)} NEAR`;
  return `near contract call-function as-transaction ${receiverId} ${methodName} json-args '${JSON.stringify(args)}' prepaid-gas '${gasTgas}.0 Tgas' attached-deposit '${depText}' sign-as ${signer || "<your-account>"} network-config ${CONFIG.network} sign-with-keychain send`;
}

// ---------- amounts ----------
export function parseUnits(text, decimals) {
  const t = String(text).trim();
  if (!/^\d+(\.\d+)?$/.test(t)) throw new Error("Enter a number like 4278 or 4278.50");
  const [whole, frac = ""] = t.split(".");
  if (frac.length > decimals) throw new Error(`At most ${decimals} decimals`);
  return (BigInt(whole) * 10n ** BigInt(decimals) + BigInt(frac.padEnd(decimals, "0") || "0")).toString();
}

export function formatUnits(value, decimals, shown = 2) {
  const v = BigInt(value);
  const base = 10n ** BigInt(decimals);
  const whole = v / base;
  const frac = (v % base).toString().padStart(decimals, "0").slice(0, shown);
  return `${whole.toLocaleString("en-US")}${shown ? "." + frac : ""}`;
}

export async function sha256Hex(data) {
  const buf = data instanceof ArrayBuffer ? data : new TextEncoder().encode(String(data));
  const hash = await crypto.subtle.digest("SHA-256", buf);
  return "sha256:" + [...new Uint8Array(hash)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

export const explorerTx = (hash) => `${CONFIG.explorer}/txns/${hash}`;

// ---------- gasless signing (NEP-366 meta transactions through server/relayer.mjs) ----------
const INTENTS = { api: "https://1click.chaindefuser.com", destinationAsset: "nep141:17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1", ...(CONFIG.intents || {}) };
const RELAYER = CONFIG.relayerUrl || "http://localhost:8787";

export const GASLESS_METHODS = ["mark_ready", "draw_sample", "submit_inspection", "release", "retry_payout"];
const nearApi = () => import("https://esm.sh/near-api-js@5.1.1");
const keyName = (account) => `lot-passport-gasless:${CONFIG.network}:${CONFIG.contractId}:${account}`;
const toB64 = (bytes) => btoa(String.fromCharCode(...bytes));

export function gaslessEnabled(account) {
  try { return !!localStorage.getItem(keyName(account)); } catch { return false; }
}

/** One wallet transaction: adds a key that can only call the gasless methods on this contract, kept in this browser. */
export async function enableGasless(account) {
  if (!wallet) throw new Error("Connect a wallet first");
  const { KeyPair } = await nearApi();
  const kp = KeyPair.fromRandom("ed25519");
  const outcome = await wallet.signAndSendTransaction({
    receiverId: account,
    actions: [{
      type: "AddKey",
      params: {
        publicKey: kp.getPublicKey().toString(),
        accessKey: { permission: { receiverId: CONFIG.contractId, methodNames: GASLESS_METHODS, allowance: (ONE_NEAR / 4n).toString() } },
      },
    }],
  });
  if (outcome?.status?.Failure) throw new Error("Adding the key failed: " + JSON.stringify(outcome.status.Failure));
  localStorage.setItem(keyName(account), kp.toString());
  return outcome?.transaction?.hash || null;
}

/** Signs a delegate action with the limited key and lets the relayer pay the gas. Returns the transaction hash. */
export async function gaslessCall(account, methodName, args, gasTgas = 100) {
  const secret = localStorage.getItem(keyName(account));
  if (!secret) throw new Error("Gasless signing is not set up on this device");
  const { KeyPair, transactions } = await nearApi();
  const kp = KeyPair.fromString(secret);
  const pk = kp.getPublicKey();
  const [ak, block] = await Promise.all([
    rpc("query", { request_type: "view_access_key", finality: "final", account_id: account, public_key: pk.toString() }),
    rpc("block", { finality: "final" }),
  ]);
  const argsBytes = new TextEncoder().encode(JSON.stringify(args));
  const gas = BigInt(gasTgas) * TGAS;
  const delegateAction = transactions.buildDelegateAction({
    senderId: account,
    receiverId: CONFIG.contractId,
    publicKey: pk,
    nonce: BigInt(ak.nonce) + 1n,
    maxBlockHeight: BigInt(block.header.height) + 120n,
    actions: [transactions.actionCreators.functionCall(methodName, argsBytes, gas, 0n)],
  });
  const message = transactions.encodeDelegateAction(delegateAction);
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", message));
  const { signature } = kp.sign(digest);
  const res = await fetch(`${RELAYER}/relay`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      senderId: account, receiverId: CONFIG.contractId, publicKey: pk.toString(),
      nonce: (BigInt(ak.nonce) + 1n).toString(), maxBlockHeight: (BigInt(block.header.height) + 120n).toString(),
      actions: [{ methodName, argsBase64: toB64(argsBytes), gas: gas.toString(), deposit: "0" }],
      signature: toB64(signature),
    }),
  });
  const j = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(j.error || `Relayer answered ${res.status}`);
  if (j.status?.Failure) throw new Error("Transaction failed: " + JSON.stringify(j.status.Failure));
  return j.hash;
}

// ---------- NEAR Intents dry quote (mainnet, preview only) ----------
export async function intentsTokens() {
  const r = await fetch(`${INTENTS.api}/v0/tokens`);
  if (!r.ok) throw new Error(`1Click tokens: ${r.status}`);
  return r.json();
}

/** Asks 1Click what it would cost to deliver `amountOut` USDC on NEAR from `originAsset`. Never opens a swap. */
export async function intentsQuote({ originAsset, amountOut, recipient }) {
  const body = {
    dry: true,
    swapType: "EXACT_OUTPUT",
    slippageTolerance: 100,
    originAsset,
    depositType: "INTENTS",
    destinationAsset: INTENTS.destinationAsset,
    amount: amountOut,
    refundTo: recipient,
    refundType: "INTENTS",
    recipient,
    recipientType: "DESTINATION_CHAIN",
    deadline: new Date(Date.now() + 10 * 60 * 1000).toISOString(),
  };
  const attempt = async (url) => {
    const r = await fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
    const j = await r.json().catch(() => ({}));
    if (!r.ok) throw new Error(j.message || j.error || `Quote failed (${r.status})`);
    return j;
  };
  try {
    return await attempt(`${INTENTS.api}/v0/quote`);
  } catch (direct) {
    // The browser may be blocked by CORS; the relayer proxies dry quotes only.
    try { return await attempt(`${RELAYER}/quote`); } catch { throw direct; }
  }
}
export const explorerAccount = (id) => `${CONFIG.explorer}/address/${id}`;

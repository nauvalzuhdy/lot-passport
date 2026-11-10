// What the relayer agrees to pay gas for. Pure functions: tested with `node --test`.

export const GASLESS_METHODS = ["mark_ready", "draw_sample", "submit_inspection", "release", "retry_payout"];
const MAX_GAS = 150n * 10n ** 12n; // 150 Tgas per action
const MAX_ACTIONS = 2;

/**
 * Checks a delegate action request before the relayer wraps and pays for it.
 * req: { senderId, receiverId, publicKey, nonce, maxBlockHeight, actions: [{ methodName, argsBase64, gas, deposit }], signature }
 * Returns { ok: true } or { ok: false, reason }.
 */
export function checkDelegate(req, { contractId, currentHeight }) {
  if (!req || typeof req !== "object") return { ok: false, reason: "Body must be a JSON object" };
  for (const k of ["senderId", "receiverId", "publicKey", "nonce", "maxBlockHeight", "signature"]) {
    if (req[k] === undefined || req[k] === null || req[k] === "") return { ok: false, reason: `Missing ${k}` };
  }
  if (req.receiverId !== contractId) return { ok: false, reason: `Only calls to ${contractId} are sponsored` };
  if (req.senderId === contractId) return { ok: false, reason: "The contract cannot be the sender" };
  if (!/^ed25519:[1-9A-HJ-NP-Za-km-z]{32,50}$/.test(req.publicKey)) return { ok: false, reason: "publicKey must be ed25519:<base58>" };
  if (!Array.isArray(req.actions) || req.actions.length === 0 || req.actions.length > MAX_ACTIONS) {
    return { ok: false, reason: `Between 1 and ${MAX_ACTIONS} actions` };
  }
  for (const a of req.actions) {
    if (!GASLESS_METHODS.includes(a.methodName)) return { ok: false, reason: `Method ${a.methodName} is not sponsored` };
    if (String(a.deposit ?? "0") !== "0") return { ok: false, reason: "Sponsored calls cannot attach a deposit" };
    let gas;
    try { gas = BigInt(a.gas); } catch { return { ok: false, reason: "gas must be an integer string" }; }
    if (gas <= 0n || gas > MAX_GAS) return { ok: false, reason: "gas out of range" };
    if (typeof a.argsBase64 !== "string") return { ok: false, reason: "argsBase64 must be a string" };
  }
  const maxH = BigInt(req.maxBlockHeight);
  if (currentHeight !== undefined) {
    const h = BigInt(currentHeight);
    if (maxH <= h) return { ok: false, reason: "Delegate action expired" };
    if (maxH > h + 1000n) return { ok: false, reason: "maxBlockHeight too far ahead" };
  }
  return { ok: true };
}

/** Quote proxy: only dry quotes pass, so the relayer can never be used to open a real swap. */
export function checkQuote(body) {
  if (!body || body.dry !== true) return { ok: false, reason: "Only dry quotes are proxied" };
  return { ok: true };
}

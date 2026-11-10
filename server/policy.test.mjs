import test from "node:test";
import assert from "node:assert/strict";
import { checkDelegate, checkQuote } from "./policy.mjs";

const base = () => ({
  senderId: "v-trust.testnet",
  receiverId: "lot-passport.testnet",
  publicKey: "ed25519:8hSHprDq2StXwMtNd43wDTXQYsjXcD4MJTXQYsjXcD4M",
  nonce: "12",
  maxBlockHeight: "1100",
  signature: "abc",
  actions: [{ methodName: "submit_inspection", argsBase64: "e30=", gas: "100000000000000", deposit: "0" }],
});
const ctx = { contractId: "lot-passport.testnet", currentHeight: 1000 };

test("accepts a sponsored inspector call", () => assert.deepEqual(checkDelegate(base(), ctx), { ok: true }));
test("refuses other contracts", () => {
  const r = base(); r.receiverId = "usdc.testnet";
  assert.equal(checkDelegate(r, ctx).ok, false);
});
test("refuses methods that move tokens or need deposits", () => {
  for (const m of ["create_lot", "register_company", "nft_transfer", "ft_transfer_call"]) {
    const r = base(); r.actions[0].methodName = m;
    assert.equal(checkDelegate(r, ctx).ok, false, m);
  }
});
test("refuses attached deposit", () => {
  const r = base(); r.actions[0].deposit = "1";
  assert.equal(checkDelegate(r, ctx).ok, false);
});
test("refuses too much gas and too many actions", () => {
  const r = base(); r.actions[0].gas = String(400n * 10n ** 12n);
  assert.equal(checkDelegate(r, ctx).ok, false);
  const r2 = base(); r2.actions = [r2.actions[0], r2.actions[0], r2.actions[0]];
  assert.equal(checkDelegate(r2, ctx).ok, false);
});
test("refuses expired or far-future delegate actions", () => {
  const r = base(); r.maxBlockHeight = "999";
  assert.equal(checkDelegate(r, ctx).ok, false);
  const r2 = base(); r2.maxBlockHeight = "5000";
  assert.equal(checkDelegate(r2, ctx).ok, false);
});
test("quote proxy only passes dry quotes", () => {
  assert.equal(checkQuote({ dry: true }).ok, true);
  assert.equal(checkQuote({ dry: false }).ok, false);
});

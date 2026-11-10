import { CONFIG } from "./config.js";
import {
  view, initWallet, connect, disconnect, call, cliCommand, parseUnits, formatUnits, sha256Hex,
  explorerTx, explorerAccount, ONE_NEAR, GASLESS_METHODS, gaslessEnabled, enableGasless, gaslessCall,
  intentsTokens, intentsQuote,
} from "./near.js";

const INCOTERMS = ["EXW", "FCA", "FAS", "FOB", "CFR", "CIF", "CPT", "CIP", "DAP", "DPU", "DDP"];
const STATUS = {
  Created: ["Waiting for payment", "hold"], Funded: ["Money locked · in production", "hold"],
  ReadyForInspection: ["Ready for inspection", ""], Passed: ["Passed · 10% held", "pass"],
  Rejected: ["Rejected · refunded", "fail"], Completed: ["Completed", "pass"],
};
const S = { account: null, token: CONFIG.token, company: undefined, balance: null, lots: [], media: {}, lookups: {} };
const $ = (id) => document.getElementById(id);
const esc = (t) => String(t ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const amount = (v) => `${formatUnits(v, S.token.decimals)} ${esc(S.token.symbol)}`;

function toast(html, ms = 9000) {
  const t = $("toast");
  t.innerHTML = html;
  t.classList.add("show");
  clearTimeout(toast.t);
  toast.t = setTimeout(() => t.classList.remove("show"), ms);
}

async function send(receiver, method, args, opts, label) {
  try {
    toast(`Waiting for your wallet to sign: ${esc(label)}…`, 60000);
    const useGasless = receiver === CONFIG.contractId && GASLESS_METHODS.includes(method) && gaslessEnabled(S.account);
    const hash = useGasless ? await gaslessCall(S.account, method, args, opts?.gasTgas || 100) : await call(receiver, method, args, opts);
    if (useGasless) label += " (gas paid by relayer)";
    toast(hash ? `${esc(label)}: done. <a href="${explorerTx(hash)}" target="_blank" rel="noopener">View proof on the explorer</a>` : `${esc(label)}: sent.`);
    await load();
  } catch (e) {
    toast(`${esc(label)} failed: ${esc(e.message || e)}`, 15000);
  }
}

// ---------- data ----------
async function load() {
  if (!CONFIG.contractId) return render();
  try {
    S.token = (await view("payment_token")) || CONFIG.token;
  } catch (e) {
    S.error = `Could not read the contract ${CONFIG.contractId}: ${e.message}`;
    return render();
  }
  S.error = null;
  if (S.account) {
    const [company, lots, balance] = await Promise.all([
      view("get_company", { account_id: S.account }),
      view("lots_for_account", { account_id: S.account }),
      view("ft_balance_of", { account_id: S.account }, S.token.account_id).catch(() => null),
    ]);
    S.company = company;
    S.lots = (lots || []).sort((a, b) => b.token_id.localeCompare(a.token_id));
    S.balance = balance;
    await Promise.all(S.lots.map(async (l) => {
      const t = await view("nft_token", { token_id: l.token_id });
      S.media[l.token_id] = t?.metadata?.media;
    }));
  }
  render();
}

async function lookup(account, slot) {
  if (!account) { S.lookups[slot] = null; return paintLookup(slot); }
  try {
    const c = await view("get_company", { account_id: account });
    S.lookups[slot] = c ? { ok: true, text: `${c.legal_name} · ${c.city ? c.city + ", " : ""}${c.country}${c.verified ? " · verified" : " · not verified yet"}` } : { ok: false, text: "This account has not registered a company yet" };
  } catch {
    S.lookups[slot] = { ok: false, text: "Account not found on " + CONFIG.network };
  }
  paintLookup(slot);
}
function paintLookup(slot) {
  const el = $("lk-" + slot);
  const l = S.lookups[slot];
  if (el) el.innerHTML = l ? `<span class="${l.ok ? "ok" : "bad"}">${esc(l.text)}</span>` : "";
}

// ---------- views ----------
function render() {
  $("net").textContent = `${CONFIG.network} · contract ${CONFIG.contractId || "not set"}`;
  $("walletBtn").textContent = S.account ? `${S.account} · Disconnect` : "Connect wallet";
  const app = $("app");
  if (!CONFIG.contractId) {
    app.innerHTML = `<section class="notice"><h2>Set your contract first</h2><p>Deploy <span class="mono">contract/</span> to testnet (README step 2), then put its account in <span class="mono">web/config.js</span> as <span class="mono">contractId</span> and reload. Meanwhile, <a href="./demo.html">the offline demo</a> shows the whole flow.</p></section>`;
    return;
  }
  if (S.error) { app.innerHTML = `<section class="notice"><h2>Contract not reachable</h2><p>${esc(S.error)}</p></section>`; return; }
  if (!S.account) {
    app.innerHTML = `<section class="card"><h2>Connect a NEAR wallet</h2><p class="lead">Use a ${esc(CONFIG.network)} account. Every action in this app is a real transaction you can open on the explorer.</p><div class="row"><button type="button" class="primary" data-act="connect">Connect wallet</button><a href="./demo.html">Or try the offline demo</a></div></section>`;
    return;
  }
  app.innerHTML = `
    <div class="grid2">${companyCard()}${tokenCard()}</div>
    ${S.company ? findCard() : ""}
    ${S.company ? dealCard() : ""}
    <section class="card" aria-labelledby="lotsH"><div class="row" style="justify-content:space-between"><h2 id="lotsH">Your lots</h2><div class="row"><button type="button" data-act="csv" ${S.lots.length ? "" : "disabled"}>Export CSV for your accountant</button><button type="button" data-act="reload">Refresh</button></div></div>
      ${S.lots.length ? S.lots.map(lotCard).join("") : `<p class="lead">No lots yet. Create one above, or ask your buyer to create one with your account as supplier or inspector.</p>`}
    </section>`;
}

function companyCard() {
  if (S.company) {
    const c = S.company;
    return `<section class="card"><h2>Your company</h2>
      <div class="facts"><div><small>Legal name</small><b>${esc(c.legal_name)}</b></div><div><small>Country</small><b>${esc(c.country)}</b></div><div><small>City</small><b>${esc(c.city || "—")}</b></div></div>
      <p>${c.verified ? `<span class="chip pass">Verified by operator</span>` : `<span class="chip hold">Not verified yet</span> <span class="hint">The contract operator verifies with <span class="mono">verify_company</span> after checking documents.</span>`}</p>
      <p class="hint">Account <a href="${explorerAccount(S.account)}" target="_blank" rel="noopener">${esc(S.account)}</a>. Everything here is public on chain.</p></section>`;
  }
  return `<section class="card"><h2>Register your company</h2>
    <p class="lead">Buyers, suppliers and inspectors each register once, so every passport shows who traded with whom.</p>
    <form class="form" id="companyForm">
      <label class="wide">Legal name<input name="legal_name" required minlength="2" maxlength="120" placeholder="Nordlicht Rechenzentren GmbH"></label>
      <label>Country (ISO code)<input name="country" required pattern="[A-Za-z]{2}" maxlength="2" placeholder="DE"></label>
      <label>City<input name="city" maxlength="80" placeholder="Frankfurt"></label>
      <label class="wide">Registration number (optional)<input name="reg" placeholder="HRB, USCC, KvK, company number…"><span class="hint">Only its SHA-256 hash goes on chain, never the number itself.</span></label>
      <label class="wide">What you make or trade (HS code prefixes, optional)<input name="hs" placeholder="8517, 8504"><span class="hint">Suppliers: buyers find you by these. Your passports are what prove it.</span></label>
      <div class="wide row"><button type="submit" class="primary">Register (0.02 NEAR storage)</button></div>
    </form></section>`;
}

function tokenCard() {
  const isWrap = /^wrap\./.test(S.token.account_id);
  return `<section class="card"><h2>Payment token</h2>
    <div class="facts"><div><small>Token</small><b class="mono">${esc(S.token.account_id)}</b></div><div><small>Symbol</small><b>${esc(S.token.symbol)}</b></div><div><small>Your balance</small><b>${S.balance == null ? "not registered" : amount(S.balance)}</b></div></div>
    <p class="hint">To send or receive ${esc(S.token.symbol)}, your account must be registered with the token once.</p>
    <div class="row"><button type="button" data-act="tokenRegister">Register with token</button>
    ${isWrap ? `<label style="display:flex;flex-direction:row;align-items:center;gap:8px">Wrap <input id="wrapAmt" value="5" style="width:90px" inputmode="decimal"> NEAR</label><button type="button" data-act="wrap">Get wNEAR</button>` : ""}</div>
    ${isWrap ? `<p class="hint">On testnet, wNEAR stands in for a stablecoin: free NEAR from the faucet, wrapped 1:1. On mainnet you would deploy with USDC.</p>` : ""}
    ${S.token.symbol === "USDC" ? `<p class="hint">Circle's test USDC: register with the token above, then request USDC for your account at <a href="https://faucet.circle.com" target="_blank" rel="noopener">faucet.circle.com</a> (network: NEAR Testnet).</p>` : ""}
    <div class="gasbox"><b>Gasless signing on this device</b>
      ${gaslessEnabled(S.account)
        ? `<span class="chip pass">On</span><span class="hint">Mark ready, draw, stamp, release and retry are signed here with a key that can only call this contract, and the relayer pays the gas.</span>`
        : `<span class="hint">For suppliers and inspectors who hold no NEAR. Adds one key limited to ${GASLESS_METHODS.join(", ")} on this contract. Needs the relayer in <span class="mono">server/</span>.</span><div class="row"><button type="button" data-act="gasless">Turn on gasless signing</button></div>`}
    </div></section>`;
}

const PRESETS = [["851762", "800G optical transceivers"], ["850440", "Server power supplies"], ["841950", "Liquid cooling cold plates"], ["847330", "Server memory modules"], ["850760", "Battery backup units"]];

function findCard() {
  const r = S.found;
  return `<section class="card" aria-labelledby="findH"><h2 id="findH">Find a supplier</h2>
    <p class="lead">Ranked by what their Lot Passports prove on chain: lots passed and failed at independent inspections. Score = (passed + 1) ÷ (inspected + 2), so a newcomer starts at 50.</p>
    <form class="form" id="findForm">
      <label>Product (HS code prefix)<input name="hs" list="hsPresets" placeholder="851762" value="${esc(S.findHs || "")}"><datalist id="hsPresets">${PRESETS.map(([c, n]) => `<option value="${c}">${n}</option>`).join("")}</datalist></label>
      <label>Supplier country<input name="country" maxlength="2" placeholder="CN" value="${esc(S.findCountry || "CN")}"></label>
      <div class="wide row"><button type="submit" class="primary">Search</button></div>
    </form>
    ${r ? (r.length ? r.map((x) => `<div class="lotrow"><div style="min-width:0"><b>${esc(x.company.legal_name)}</b> <span class="hint">${esc(x.company.city)}, ${esc(x.company.country)} · <span class="mono">${esc(x.company.account_id)}</span></span><br>
        <span class="hint">${x.passed + x.rejected ? `${x.passed} passed · ${x.rejected} failed` : "No inspected lots yet"} · ${x.defect_ppm == null ? "no sample data" : (x.defect_ppm / 10000).toFixed(1) + "% defects in samples"} · ${Number(x.units_passed).toLocaleString("en-US")} units passed${x.company.verified ? " · verified" : ""}</span></div>
        <div class="row"><b style="font-size:22px">${Math.round(x.score_bp / 100)}</b><button type="button" data-act="useSupplier" data-acc="${esc(x.company.account_id)}">Use in a new deal</button></div></div>`).join("") : `<p class="hint">No supplier matches. Try a shorter HS prefix such as 8517.</p>`) : ""}
  </section>`;
}

function dealCard() {
  return `<section class="card" aria-labelledby="dealH"><h2 id="dealH">Start a deal: mint a Lot Passport</h2>
    <p class="lead">You are the buyer. Enter what was agreed with the supplier. The pass rule is fixed from the quantity using ISO 2859-1, AQL 2.5.</p>
    <form class="form" id="dealForm">
      <label>Supplier account<input name="supplier" required placeholder="lightwave-optics.testnet" data-lookup="supplier"><span class="hint" id="lk-supplier"></span></label>
      <label>Inspector account<input name="inspector" required placeholder="v-trust.testnet" data-lookup="inspector"><span class="hint" id="lk-inspector"></span></label>
      <label class="wide">Goods<input name="description" required minlength="2" maxlength="120" placeholder="800G OSFP optical transceivers"></label>
      <label>HS code<input name="hs_code" required pattern="[0-9]{6,10}" placeholder="851762"><span class="hint">6 to 10 digits, no dots</span></label>
      <label>Incoterm<select name="incoterm">${INCOTERMS.map((i) => `<option ${i === "FOB" ? "selected" : ""}>${i}</option>`).join("")}</select></label>
      <label>Origin country<input name="origin_country" required pattern="[A-Za-z]{2}" maxlength="2" placeholder="CN"></label>
      <label>Destination country<input name="destination_country" required pattern="[A-Za-z]{2}" maxlength="2" placeholder="DE"></label>
      <label>Quantity (pieces)<input name="quantity" required inputmode="numeric" pattern="[0-9]+" placeholder="2400"></label>
      <label>Pieces per carton<input name="units_per_carton" required inputmode="numeric" pattern="[0-9]+" placeholder="20"></label>
      <label class="wide">Price in ${esc(S.token.symbol)}<input name="price" required inputmode="decimal" placeholder="${S.token.decimals === 24 ? "15" : "4278"}"><span class="hint">${S.token.decimals === 24 ? "On testnet wNEAR stands in for dollars, so use a small number." : ""}</span></label>
      <div class="wide row"><button type="submit" class="primary">Mint passport (0.05 NEAR storage)</button></div>
    </form></section>`;
}

function lotCard(l) {
  const me = S.account;
  const roles = [l.buyer === me && "buyer", l.supplier === me && "supplier", l.inspector === me && "inspector", l.owner_id === me && l.owner_id !== l.buyer && "holder"].filter(Boolean);
  const [label, tone] = STATUS[l.status] || [l.status, ""];
  const img = S.media[l.token_id];
  return `<article class="lot">
    <div>${img ? `<img src="${img}" alt="Lot Passport #${esc(l.token_id)}: ${esc(l.product)}, ${esc(label)}">` : ""}</div>
    <div style="display:grid;gap:12px;min-width:0">
      <div class="row"><h3>#${esc(l.token_id)} · ${esc(l.product)}</h3><span class="chip ${tone}">${esc(label)}</span>${roles.map((r) => `<span class="chip">you: ${r}</span>`).join("")}</div>
      <div class="facts">
        <div><small>Supplier</small><b>${esc(l.supplier_name)} (${esc(l.origin_country)})</b></div>
        <div><small>Buyer</small><b>${esc(l.buyer_name)} (${esc(l.destination_country)})</b></div>
        <div><small>Inspector</small><b>${esc(l.inspector_name)}</b></div>
        <div><small>HS code · Incoterm</small><b>${esc(l.hs_code)} · ${esc(l.incoterm)}</b></div>
        <div><small>Quantity</small><b>${Number(l.quantity).toLocaleString("en-US")} pcs</b></div>
        <div><small>Pass rule</small><b>≤ ${l.accept_max} of ${l.sample_size}</b></div>
        <div><small>Price</small><b>${amount(l.price)}</b></div>
        <div><small>Inside passport</small><b>${amount(l.held)}</b></div>
        <div><small>Paid / refunded</small><b>${amount(l.paid_to_supplier)} / ${amount(l.refunded_to_buyer)}</b></div>
      </div>
      ${l.cartons.length ? `<p>Cartons to open: <span class="mono">${l.cartons.map((c) => "#" + c).join(" · ")}</span>${l.defects != null ? ` · result <b>${l.defects} of ${l.sample_size}</b> defective` : ""}</p>` : ""}
      ${actionsFor(l, roles)}
      ${historyFor(l)}
    </div></article>`;
}

const when = (ns) => new Date(Number(BigInt(ns) / 1_000_000n)).toISOString().replace("T", " ").slice(0, 16) + " UTC";
const KIND = { funded: "Paid into passport", paid_supplier: "90% paid to supplier", refund_buyer: "Refunded to buyer", release_retention: "10% released to supplier" };

function historyFor(l) {
  const pay = (l.payments || []).map((p) => `<tr><td>${when(p.at)}</td><td>${esc(KIND[p.kind] || p.kind)}</td><td class="mono">${esc(p.to)}</td><td>${amount(p.amount)}</td><td>${p.settled ? "settled" : `<b class="bad">failed, waiting for retry</b>`}</td></tr>`).join("");
  const holders = (l.holders || []).map((h, i, a) => `<tr><td>${when(h.since)}</td><td class="mono">${esc(h.account_id)}</td><td>${i === a.length - 1 ? "current holder" : "past holder"}</td></tr>`).join("");
  const failed = (l.payments || []).some((p) => !p.settled);
  return `<details ${failed ? "open" : ""}><summary>History: payments and holders</summary>
    <div class="tbl"><table><thead><tr><th>When</th><th>What</th><th>To</th><th>Amount</th><th>State</th></tr></thead><tbody>${pay || `<tr><td colspan="5">No payments yet</td></tr>`}</tbody></table></div>
    <div class="tbl"><table><thead><tr><th>Since</th><th>Holder</th><th></th></tr></thead><tbody>${holders}</tbody></table></div>
    ${l.report_hash ? `<p class="hint">Inspection report hash: <span class="mono">${esc(l.report_hash)}</span></p>` : ""}
    ${failed ? `<div class="row"><button type="button" data-act="retry" data-id="${esc(l.token_id)}">Retry the failed payout</button></div><p class="hint">The receiver was probably not registered with the token. Register it, then retry: money only goes to the recorded receiver.</p>` : ""}
  </details>`;
}

function csvExport() {
  const cell = (v) => { const t = String(v ?? ""); return /[",\n]/.test(t) ? `"${t.replace(/"/g, '""')}"` : t; };
  const head = ["date_utc", "lot", "goods", "hs_code", "incoterm", "origin", "destination", "supplier", "buyer", "inspector", "movement", "from", "to", `amount_${S.token.symbol}`, "settled", "lot_status", "inspection", "explorer_account"];
  const rows = [];
  for (const l of S.lots) {
    const insp = l.defects == null ? "" : `${l.defects} of ${l.sample_size} defective (limit ${l.accept_max})`;
    const base = [l.token_id, l.product, l.hs_code, l.incoterm, l.origin_country, l.destination_country, l.supplier_name, l.buyer_name, l.inspector_name];
    if (!(l.payments || []).length) rows.push([when(l.created_at), ...base, "minted, not paid", "", "", "0", "", l.status, insp, explorerAccount(CONFIG.contractId)]);
    for (const p of l.payments || []) {
      rows.push([when(p.at), ...base, KIND[p.kind] || p.kind, p.from, p.to, formatUnits(p.amount, S.token.decimals, S.token.decimals).replace(/,/g, ""), p.settled ? "yes" : "no", l.status, insp, explorerAccount(CONFIG.contractId)]);
    }
  }
  const csv = [head, ...rows].map((r) => r.map(cell).join(",")).join("\n");
  const a = document.createElement("a");
  a.href = URL.createObjectURL(new Blob([csv], { type: "text/csv" }));
  a.download = `lot-passports-${S.account}-${new Date().toISOString().slice(0, 10)}.csv`;
  a.click();
  URL.revokeObjectURL(a.href);
}

function intentsPanel(l) {
  return `<details data-intents="${esc(l.token_id)}"><summary>Paying from another chain? Preview a NEAR Intents route</summary>
    <p class="hint">NEAR Intents has no testnet, so this asks 1Click for a <b>dry quote on mainnet</b>: what it would cost to deliver ${amount(l.price)} as USDC on NEAR from the token you hold. Nothing is sent.</p>
    <div class="form">
      <label>Pay with<select data-intents-origin><option value="">Load tokens first</option></select></label>
      <label>Your mainnet NEAR account (receives refunds in a real swap)<input data-intents-recipient placeholder="yourname.near"></label>
      <div class="wide row"><button type="button" data-act="intentsTokens">Load tokens</button><button type="button" data-act="intentsQuote" data-id="${esc(l.token_id)}">Get dry quote</button></div>
    </div><div class="hint" data-intents-out></div></details>`;
}

function actionsFor(l, roles) {
  const id = esc(l.token_id);
  const out = [];
  const cli = (receiver, method, args, opts) => `<details><summary>Do it from the terminal instead</summary><pre>${esc(cliCommand(S.account, receiver, method, args, opts))}</pre></details>`;
  if (roles.includes("buyer") && l.status === "Created") {
    out.push(`<div class="row"><button type="button" class="primary" data-act="pay" data-id="${id}">Pay ${amount(l.price)} into the passport</button></div>
      <p class="hint">Your wallet signs <span class="mono">ft_transfer_call</span> on ${esc(S.token.account_id)}. If the amount or sender is wrong, the contract refuses and the token returns it. The passport contract must be registered with the token (README step 2).</p>
      ${cli(S.token.account_id, "ft_transfer_call", { receiver_id: CONFIG.contractId, amount: l.price, msg: l.token_id }, { gasTgas: 100, depositYocto: "1" })}
      ${S.token.decimals === 6 ? intentsPanel(l) : ""}`);
  }
  if (roles.includes("supplier") && l.status === "Funded") {
    out.push(`<div class="row"><button type="button" class="primary" data-act="ready" data-id="${id}">Production done: call the inspector</button></div>${cli(CONFIG.contractId, "mark_ready", { token_id: l.token_id })}`);
  }
  if ((roles.includes("inspector") || roles.includes("supplier")) && l.status === "ReadyForInspection" && !l.cartons.length) {
    out.push(`<div class="row"><button type="button" class="primary" data-act="draw" data-id="${id}">Draw the cartons to open</button></div><p class="hint">Do this at the factory: the contract picks random cartons from the block's seed.</p>${cli(CONFIG.contractId, "draw_sample", { token_id: l.token_id })}`);
  }
  if (roles.includes("inspector") && l.status === "ReadyForInspection" && l.cartons.length) {
    out.push(`<form class="form" data-inspect="${id}">
      <label>Pieces checked<input name="sampled" required inputmode="numeric" value="${l.sample_size}"></label>
      <label>Problem pieces found<input name="defects" required inputmode="numeric" placeholder="0"></label>
      <label class="wide">Inspection report (PDF or photos zip)<input name="report" type="file"><span class="hint">Only its SHA-256 hash goes on chain. Keep the file: anyone can later check it matches.</span></label>
      <div class="wide row"><button type="submit" class="primary">Sign and stamp the passport</button></div></form>
      <p class="hint">The contract decides pass or fail from your counts. Pass pays the supplier 90% now; fail refunds the buyer in full.</p>`);
  }
  if (roles.includes("buyer") && l.status === "Passed") {
    out.push(`<div class="row"><button type="button" class="primary" data-act="release" data-id="${id}">Goods arrived fine: release the last 10%</button></div><p class="hint">Anyone can release it after the 7-day claim window.</p>${cli(CONFIG.contractId, "release", { token_id: l.token_id })}`);
  }
  if (l.owner_id === S.account && l.status === "Completed") {
    out.push(`<form class="row" data-transfer="${id}"><label style="flex:1 1 240px">Hand this passport to<input name="receiver" required placeholder="nauval-lp-kestrel-cloud.testnet"></label><button type="submit">Transfer passport</button></form><p class="hint">The quality record goes with it. Standard NEP-171 transfer.</p>`);
  }
  return out.join("") || `<p class="hint">Nothing for you to do on this lot right now.</p>`;
}

// ---------- events ----------
document.addEventListener("click", async (e) => {
  const b = e.target.closest("[data-act]");
  if (!b) return;
  const act = b.dataset.act, id = b.dataset.id;
  const lot = S.lots.find((l) => l.token_id === id);
  if (act === "connect") { try { S.account = await connect(); await load(); } catch (err) { toast("Wallet: " + esc(err.message || err)); } }
  if (act === "reload") await load();
  if (act === "tokenRegister") await send(S.token.account_id, "storage_deposit", { account_id: S.account, registration_only: true }, { gasTgas: 30, depositYocto: (ONE_NEAR / 80n).toString() }, "Register with token");
  if (act === "wrap") {
    try { const y = parseUnits($("wrapAmt").value, 24); await send(S.token.account_id, "near_deposit", {}, { gasTgas: 30, depositYocto: y }, "Wrap NEAR"); } catch (err) { toast(esc(err.message)); }
  }
  if (act === "pay") await send(S.token.account_id, "ft_transfer_call", { receiver_id: CONFIG.contractId, amount: lot.price, msg: lot.token_id }, { gasTgas: 100, depositYocto: "1" }, `Pay lot #${id}`);
  if (act === "ready") await send(CONFIG.contractId, "mark_ready", { token_id: id }, {}, "Mark ready");
  if (act === "draw") await send(CONFIG.contractId, "draw_sample", { token_id: id }, {}, "Draw cartons");
  if (act === "release") await send(CONFIG.contractId, "release", { token_id: id }, {}, "Release retention");
  if (act === "useSupplier") {
    const f = $("dealForm");
    f.supplier.value = b.dataset.acc;
    if (S.findHs && S.findHs.length >= 6) f.hs_code.value = S.findHs;
    lookup(b.dataset.acc, "supplier");
    f.scrollIntoView({ behavior: "smooth", block: "start" });
  }
  if (act === "retry") await send(CONFIG.contractId, "retry_payout", { token_id: id }, {}, "Retry payout");
  if (act === "csv") csvExport();
  if (act === "gasless") {
    try { toast("Approve the new limited key in your wallet…", 60000); const h = await enableGasless(S.account); toast(`Gasless signing is on for this device.${h ? ` <a href="${explorerTx(h)}" target="_blank" rel="noopener">View the key on the explorer</a>` : ""}`); render(); }
    catch (err) { toast("Could not turn on gasless signing: " + esc(err.message || err), 15000); }
  }
  if (act === "intentsTokens") {
    const box = b.closest("[data-intents]");
    try {
      const list = (await intentsTokens()).filter((t) => /^(USDC|USDT)$/.test(t.symbol)).sort((a, c) => (a.blockchain + a.symbol).localeCompare(c.blockchain + c.symbol));
      box.querySelector("[data-intents-origin]").innerHTML = list.map((t) => `<option value="${esc(t.assetId)}">${esc(t.symbol)} on ${esc(t.blockchain)}</option>`).join("");
    } catch (err) { box.querySelector("[data-intents-out]").textContent = "Could not load tokens: " + err.message; }
  }
  if (act === "intentsQuote") {
    const box = b.closest("[data-intents]");
    const out = box.querySelector("[data-intents-out]");
    const originAsset = box.querySelector("[data-intents-origin]").value;
    const recipient = box.querySelector("[data-intents-recipient]").value.trim();
    if (!originAsset || !recipient) { out.textContent = "Load tokens, pick one, and enter a mainnet account first."; return; }
    out.textContent = "Asking 1Click…";
    try {
      const q = await intentsQuote({ originAsset, amountOut: lot.price, recipient });
      const r = q.quote || q;
      out.innerHTML = `<b>Dry quote</b> · send about <b>${esc(r.amountInFormatted ?? r.amountIn)}</b>${r.amountInUsd ? ` (≈ $${esc(r.amountInUsd)})` : ""} to receive <b>${esc(r.amountOutFormatted ?? r.amountOut)} USDC</b> on NEAR${r.timeEstimate ? `, about ${esc(r.timeEstimate)} s` : ""}. Without a 1Click key a 0.2% fee applies.`;
    } catch (err) { out.textContent = "Quote failed: " + err.message; }
  }
});

document.addEventListener("submit", async (e) => {
  e.preventDefault();
  const f = e.target;
  const v = Object.fromEntries(new FormData(f).entries());
  try {
    if (f.id === "findForm") {
      S.findHs = v.hs.replace(/\D/g, "");
      S.findCountry = v.country.trim().toUpperCase();
      S.found = await view("find_suppliers", { hs_prefix: S.findHs || null, country: S.findCountry || null, limit: 20 });
      render();
      return;
    }
    if (f.id === "companyForm") {
      const reg = v.reg.trim() ? await sha256Hex(v.reg.trim()) : null;
      const hs = (v.hs || "").split(/[\s,;]+/).map((x) => x.replace(/\D/g, "")).filter(Boolean);
      await send(CONFIG.contractId, "register_company", { legal_name: v.legal_name.trim(), country: v.country.toUpperCase(), city: v.city.trim(), registration_hash: reg, hs_codes: hs }, { gasTgas: 30, depositYocto: (ONE_NEAR / 50n).toString() }, "Register company");
    }
    if (f.id === "dealForm") {
      const args = {
        supplier: v.supplier.trim(), inspector: v.inspector.trim(),
        goods: {
          description: v.description.trim(), hs_code: v.hs_code.trim(), quantity: Number(v.quantity), units_per_carton: Number(v.units_per_carton),
          origin_country: v.origin_country.toUpperCase(), destination_country: v.destination_country.toUpperCase(), incoterm: v.incoterm,
        },
        price: parseUnits(v.price, S.token.decimals),
      };
      await send(CONFIG.contractId, "create_lot", args, { gasTgas: 50, depositYocto: (ONE_NEAR / 20n).toString() }, "Mint Lot Passport");
    }
    if (f.dataset.inspect) {
      const file = f.querySelector('input[type="file"]').files[0];
      const hash = file ? await sha256Hex(await file.arrayBuffer()) : await sha256Hex(`no-file:${f.dataset.inspect}:${Date.now()}`);
      await send(CONFIG.contractId, "submit_inspection", { token_id: f.dataset.inspect, sampled: Number(v.sampled), defects: Number(v.defects), report_hash: hash }, { gasTgas: 100 }, "Stamp passport");
    }
    if (f.dataset.transfer) {
      await send(CONFIG.contractId, "nft_transfer", { receiver_id: v.receiver.trim(), token_id: f.dataset.transfer }, { gasTgas: 30, depositYocto: "1" }, "Transfer passport");
    }
  } catch (err) {
    toast(esc(err.message || err), 12000);
  }
});

document.addEventListener("change", (e) => {
  const slot = e.target.dataset?.lookup;
  if (slot) lookup(e.target.value.trim(), slot);
});

$("walletBtn").addEventListener("click", async () => {
  if (S.account) { await disconnect(); S.account = null; S.company = undefined; S.lots = []; render(); return; }
  try { S.account = await connect(); await load(); } catch (err) { toast("Wallet: " + esc(err.message || err)); }
});

render();
if (CONFIG.contractId) {
  initWallet(async (acc) => { S.account = acc; await load(); });
  load();
}

# Lot Passport

**Every batch of goods traded between two companies gets a passport that holds its own payment.** An independent inspector stamps it at the factory before anything ships. Pass: the supplier is paid. Fail: the goods stay at the factory and the buyer gets everything back. Afterwards the passport is a transferable NFT that proves the lot's quality to anyone.

Example used throughout: a data-center operator in Frankfurt buys AI hardware (2,400 800G optical transceivers) from a factory in Shenzhen, shipped FOB to Hamburg.

Built at NEAR HACKATHON (November 2026). Tracks: **NFTs That Do Something** (`nfts`) and **Real-World Money** (`realworld`).

## Live on NEAR testnet

| | |
|---|---|
| Contract | [`nauval-lp-contract.testnet`](https://testnet.nearblocks.io/address/nauval-lp-contract.testnet) |
| Payment token | Circle test USDC, [`3e2210e1…4afcb8af`](https://testnet.nearblocks.io/address/3e2210e1184b45b64c8a434c0a7e7b23cc04ea7eb7a6c3c32520d03d4afcb8af) (6 decimals) |
| Buyer | [`nauval-lp-nordlicht-rz.testnet`](https://testnet.nearblocks.io/address/nauval-lp-nordlicht-rz.testnet) |
| Supplier | [`nauval-lp-lightwave-optics.testnet`](https://testnet.nearblocks.io/address/nauval-lp-lightwave-optics.testnet) |
| Inspector | [`nauval-lp-v-trust.testnet`](https://testnet.nearblocks.io/address/nauval-lp-v-trust.testnet) |
| Customer | [`nauval-lp-kestrel-cloud.testnet`](https://testnet.nearblocks.io/address/nauval-lp-kestrel-cloud.testnet) |

**Read a finished passport in the browser, no wallet needed.** Run `bash scripts/serve.sh`, then open:

- [`localhost:5173/?lot=0001`](http://localhost:5173/?lot=0001) — inspection passed, supplier paid in full
- [`localhost:5173/?lot=0002`](http://localhost:5173/?lot=0002) — inspection failed, buyer refunded in full

Both pages are view calls only: the NFT card, status, the three companies, every payment and every holder, read straight off the chain with nothing to sign. The same paths work on the hosted copy (`https://<user>.github.io/<repo>/?lot=0001`).

Read it without installing anything — this returns the USDC token the contract settles in:

```
curl -s -X POST https://rpc.testnet.fastnear.com -H 'Content-Type: application/json' -d '{"jsonrpc":"2.0","id":1,"method":"query","params":{"request_type":"call_function","finality":"final","account_id":"nauval-lp-contract.testnet","method_name":"payment_token","args_base64":"e30="}}'
```

## Proof on chain

Every step below is a real testnet transaction against `nauval-lp-contract.testnet`. The two **Refused** rows failed on purpose — that is the contract rejecting a cheat, not a broken demo.

### Lot `0001` — inspection passes, supplier is paid

Read it in the browser with no wallet: [`?lot=0001`](http://localhost:5173/?lot=0001)

| Step | Signed by | Transaction | Result |
|---|---|---|---|
| Deploy + `new` init | contract | [`3iSDkMGrtw…`](https://testnet.nearblocks.io/txns/3iSDkMGrtwbWpY32pNehBrd3TgEH6ApfxipB3yjeSD1o) | USDC set as the settlement token |
| Mint the passport (`create_lot`) | buyer | [`56VVUj72Bz…`](https://testnet.nearblocks.io/txns/56VVUj72BzK5PNDzTkJX6g3jN4idPA9c8ubTdQcuoPwS) | lot `0001`, 2,400 modules, 15 USDC |
| Pay into it (`ft_transfer_call`) | buyer | [`AUPGYEscXU…`](https://testnet.nearblocks.io/txns/AUPGYEscXUbx2GyamnJcvaXUawnnpv2iYGDjMbGABH85) | 15 USDC locked inside the passport |
| Production done (`mark_ready`) | supplier | [`9fubdn4vYe…`](https://testnet.nearblocks.io/txns/9fubdn4vYess4Ron7qikp5Lnp3wCJoDKGbYFnUcxS9bP) | calls the inspector |
| Draw the cartons (`draw_sample`) | inspector | [`21KWgWkbQL…`](https://testnet.nearblocks.io/txns/21KWgWkbQLfhpQYQT3jvoFUeu7E1rahgj5PVwWctVuF3) | block seed picked 30, 43, 51, 94, 110, 114 |
| **Refused:** buyer tries to stamp | buyer | [`XZ2eQ4AEzm…`](https://testnet.nearblocks.io/txns/XZ2eQ4AEzmP7TqxEsKdJ8VzaHJntMkE2Va2MBPdRb5Z) | `Only the agreed inspector can stamp this passport` |
| Stamp it (`submit_inspection`) | inspector | [`JAEjvvDUhM…`](https://testnet.nearblocks.io/txns/JAEjvvDUhMNmAMXxRtiCB4CCCy9BJFu3f2T5nuEeaPwi) | 2 defects in 125, limit 7 → pass, 90% paid |
| **Refused:** sell while money is inside | buyer | [`Dy3ZoBKT3A…`](https://testnet.nearblocks.io/txns/Dy3ZoBKT3AGEmKGV8jBTedep1tcMZEq8DcVFsKxYuXBe) | `…only after its lot is settled and nothing is left inside` |
| Release the last 10% (`release`) | buyer | [`9fYDNXYd9Q…`](https://testnet.nearblocks.io/txns/9fYDNXYd9QjnTwK1FcjN1jEYUskdEf8yMjZS3PWLaiCA) | supplier now holds all 15 USDC |
| Transfer the passport (`nft_transfer`) | buyer | [`HsBHfrp9ko…`](https://testnet.nearblocks.io/txns/HsBHfrp9koRwfJfBgYqkWiWg9QEHNWoFXJyKgf5LnhWr) | passport moves to the customer |

Net effect: buyer −15 USDC, supplier +15 USDC (13.5 on the stamp, 1.5 on release), contract holds nothing.

### Lot `0002` — inspection fails, buyer gets everything back

Read it in the browser with no wallet: [`?lot=0002`](http://localhost:5173/?lot=0002)

| Step | Signed by | Transaction | Result |
|---|---|---|---|
| Mint (`create_lot`) | buyer | [`HUDcjWeSJG…`](https://testnet.nearblocks.io/txns/HUDcjWeSJGTyHCYMsWZ5yxaRF7dAXUDDovuyMkDUB6mR) | lot `0002`, 800 modules, 5 USDC |
| Pay (`ft_transfer_call`) | buyer | [`G15VwPmPTE…`](https://testnet.nearblocks.io/txns/G15VwPmPTEoGGNL1fRT572PxMcADVB4hKwxT2PaFHwDx) | 5 USDC locked, buyer down to 0 |
| Production done (`mark_ready`) | supplier | [`3oqqSFuXev…`](https://testnet.nearblocks.io/txns/3oqqSFuXevsNZDiprtmEkepSTYp9yEStMevz7KLHv85y) |  |
| Draw (`draw_sample`) | inspector | [`BdH9FQUftB…`](https://testnet.nearblocks.io/txns/BdH9FQUftBKRJuBe2ReLUmh2tkPA46F2Pg5StZkqHFtN) | 80 of 800 to be opened |
| Stamp it (`submit_inspection`) | inspector | [`Gza8PLDzNJ…`](https://testnet.nearblocks.io/txns/Gza8PLDzNJN9yjk9xf5sgNthsRCrjuk4k24oWwyqPTCw) | **12 defects in 80, limit 5 → fail, full refund** |

Net effect: the buyer paid 5 USDC, the batch failed, and the 5 USDC came straight back. The supplier was paid **nothing**. Read the final state yourself:

```
curl -s -X POST https://rpc.testnet.fastnear.com -H 'Content-Type: application/json' -d '{"jsonrpc":"2.0","id":1,"method":"query","params":{"request_type":"call_function","finality":"final","account_id":"nauval-lp-contract.testnet","method_name":"get_lot","args_base64":"eyJ0b2tlbl9pZCI6IjAwMDIifQ=="}}'
```

It returns `"status": "Rejected"`, `"paid_to_supplier": "0"`, `"held": "0"`, and a `refund_buyer` payment of `5000000`.


## The problem

In cross-border B2B trade the buyer fears paying upfront and the supplier fears shipping first. Letters of credit solve it slowly and expensively, out of reach for small companies. Marketplace escrow only works inside one marketplace and settles disputes by hand. Neither ties the money to an objective quality check.

## What it does

1. **Companies register** their legal name, country, city and, for suppliers, the HS code prefixes they make (`register_company`). The operator can mark a profile verified after checking documents (`verify_company`).
2. **The buyer finds a supplier** (`find_suppliers`) by HS code and country. Results are ranked by what the supplier's past passports prove: lots passed and failed at independent inspections, defect rate in samples, units shipped. The score is a smoothed pass rate, (passed + 1) / (inspected + 2), so a newcomer starts at 50 and one lucky lot cannot jump to the top. Only inspected lots count.
3. **The buyer mints a Lot Passport** (`create_lot`) for an agreed deal: goods, HS code, quantity, origin and destination country, Incoterm, price, supplier and inspector. The contract fixes the sampling plan from the quantity with **ISO 2859-1, inspection level II, AQL 2.5**: 2,400 modules means test 125, pass if 7 or fewer fail. Nobody can change the rule later.
4. **The buyer pays into the passport** with `ft_transfer_call` on USDC. Wrong amount, wrong payer or a second payment: the contract refuses and the token standard returns the money.
5. **The supplier produces**, then calls `mark_ready`.
6. **At the factory the contract draws the cartons** to open from the block's random seed (`draw_sample`), so the supplier cannot prepare them.
7. **Only the agreed inspector can stamp the passport** (`submit_inspection`). The inspector sends counts and a report hash; **the contract decides** pass or fail. Pass: 90% goes to the supplier now and 10% stays inside for a 7-day claim window. Fail: the buyer is refunded in full and nothing ships.
8. **`release`** pays the 10%: the buyer can release early, anyone can after 7 days.
9. **`nft_transfer`** hands the passport and its record to a new holder, for example the cloud provider that leases the racks.

The NFT's `media` is an SVG card **drawn by the contract from live state**: route, HS code, money inside and a stamp that changes from MONEY LOCKED to PASSED to COMPLETED. Wallets show each lot as that card.

## What happens on transfer

- A passport can change hands **only after its lot is settled** (Completed or Rejected), **with nothing held inside and no payout pending**. So no money ever travels with the token, and a sale can never strand a payout (`can_transfer` in `contract/src/core.rs`).
- The new holder gets **the record**: inspection result, signer, report hash, every payment, every past holder. The old holder keeps nothing that the token controls, because the token controls no money at that point.
- The **roles never move**: buyer, supplier and inspector stay as recorded at mint. Selling the passport cannot make anyone the payee.
- `nft_transfer_call` follows NEP-171: if the receiving contract returns `true`, the passport goes back to the sender.
- Every holder is kept in `holders`, oldest first; the web app shows them under "History".

## How NEAR is used

| NEAR feature | Where |
| --- | --- |
| Smart contract (Rust, near-sdk 5) | Holds the money per lot, enforces roles, applies the AQL rule |
| NEP-171 core + NEP-181 enumeration + NEP-177 metadata | One token per lot; `nft_transfer`, `nft_transfer_call`, `nft_token`, `nft_tokens`, `nft_tokens_for_owner`, `nft_total_supply`, `nft_supply_for_owner`, `nft_metadata` |
| Dynamic NFT | `media` is regenerated from the lot's state on every read |
| Stablecoin (NEP-141) | Native USDC on NEAR testnet; payment through `ft_transfer_call` / `ft_on_transfer`, payouts through `ft_transfer` with a result check |
| Meta transactions (NEP-366) | Optional relayer in `server/` pays gas for suppliers and inspectors |
| Function-call access keys | Gasless mode adds a device key that can only call five methods on this contract |
| NEAR Intents (1Click API) | Dry quote on mainnet: what it costs to pay a passport from USDC or USDT on another chain |
| On-chain randomness | Carton draw |
| Supplier search from passports | `find_suppliers` aggregates every supplier's passports into a public, unbuyable track record |
| Named accounts, NEP-297 events | Readable company accounts; `nft_mint`, `nft_transfer`, `lot_funded`, `sample_drawn`, `lot_inspected`, `lot_completed`, `payout_failed`, `company_registered` |

## Try it: do X, see Y

You need Rust (stable, 1.95 or newer — `near-sdk` 5.30 will not build on older compilers) with the `wasm32-unknown-unknown` target, [cargo-near](https://github.com/near/cargo-near), [near-cli-rs](https://github.com/near/near-cli-rs), Python 3, and Node 20 for the optional relayer.

**0. Test the logic.** `bash scripts/test-core.sh` → 17 tests pass with plain `rustc` (AQL table, carton draw, verdict, 90/10 split, release and transfer rules, pagination, trade fields, track record scoring, card image). `cd contract && cargo test` → 28 tests pass (the 17 above plus 11 contract tests: registration, wrong amount refused, buyer cannot stamp, pass and fail paths, release rules, sale during claim window refused, supplier ranking, enumeration). `cd server && npm test` → relayer policy tests pass.

**1. Create testnet accounts** (free): contract, buyer, supplier, inspector, customer.
`near account create-account sponsor-by-faucet-service <name>.testnet autogenerate-new-keypair save-to-keychain network-config testnet create`

**2. Get test USDC.** Circle's USDC on NEAR testnet is `3e2210e1184b45b64c8a434c0a7e7b23cc04ea7eb7a6c3c32520d03d4afcb8af` (6 decimals). Register the buyer with it (`storage_deposit`, done by the script), then request USDC for the buyer at [faucet.circle.com](https://faucet.circle.com), network **NEAR Testnet**. The faucet gives a small amount per request, so the demo price is 15 USDC. If you have no USDC yet, set `TOKEN_KIND=wnear` in the script to use wrapped testnet NEAR instead.

**3. Run the whole flow from the terminal.** Edit the names at the top of `scripts/testnet-demo.sh`, then `bash scripts/testnet-demo.sh`.
You will see: the contract deploys; three companies register; lot `0001` is minted; 15 USDC is locked inside it; cartons are drawn; **the buyer's attempt to stamp is refused**; the inspector stamps it (2 of 125 failed, pass); 13.5 USDC reaches the supplier; **an attempt to sell the passport while 1.5 USDC is still inside is refused**; the buyer releases the last 1.5; the passport moves to the customer. The last commands print the lot with its payments and holders, and the supplier search ranked by passport record.

**4. Use the web app.** `web/config.js` already points at the live contract above; replace `contractId` if you deploy your own. Run `bash scripts/serve.sh`, open http://localhost:5173 and connect a testnet wallet. Register a company, search suppliers by HS code, mint a lot, pay, and signed in as the supplier or inspector take the next step. Each action links to its transaction on the explorer and shows the equivalent terminal command. "Export CSV" downloads every payment for an accountant. Hosted copy: the `pages` workflow publishes `web/` to GitHub Pages on every push to `main` (enable Pages with source "GitHub Actions"); it stays up through judging.

**5. Optional: gasless signing.** `cd server && npm install`, then `RELAYER_ACCOUNT=<funded testnet account> RELAYER_KEY=ed25519:<its private key> CONTRACT_ID=<contract> npm start`. In the web app, the supplier or inspector presses "Turn on gasless signing" once; from then on mark ready, draw, stamp, release and retry are paid by the relayer. The relayer refuses anything else (`server/policy.mjs`). Never commit the key.

**6. Optional: NEAR Intents preview.** On a lot waiting for payment, open "Paying from another chain?", load tokens, pick e.g. USDT on TRON and get a dry quote. NEAR Intents has no testnet, so this is a mainnet price preview only; nothing is sent.

**Offline demo.** `web/demo.html` runs the whole story in the browser with no chain.

## For bookkeepers

Each passport is one purchase. Its payments are: paid into the passport (buyer → contract), 90% paid to supplier, 10% released to supplier, or a full refund to the buyer. The CSV has one row per movement: date (UTC), lot, goods, HS code, Incoterm, origin, destination, supplier, buyer, inspector, movement, from, to, amount, settled, lot status, inspection result and a link to the public record. Amounts are in the token's units (USDC = US dollars).

## Limits, stated plainly

- Money is held by the contract and accounted per passport, not in a separate sub-account per passport.
- `env::random_seed` could in theory be biased by a block producer; see THREAT_MODEL.md #14.
- Company profiles are self-declared until the operator verifies them.
- Everything on chain is public. Registration numbers are stored only as SHA-256 hashes.
- The Intents integration is a dry quote: paying a passport cross-chain for real needs mainnet.
- Card payments are not built.
- Regulation: rules for stablecoin payments differ by country (the EU regulates stablecoins under MiCA; some countries forbid crypto as a means of payment). A real launch would use licensed partners for fiat on and off ramps.
- The supplier ranking can be gamed by a supplier trading with itself through a colluding inspector; see THREAT_MODEL.md #16.
- `find_suppliers` scans all lots on each call; a production version would keep per-supplier counters.

## Threat model

See [THREAT_MODEL.md](THREAT_MODEL.md): sixteen ways a party could keep money, goods or reputation, and which rule or test stops each.

## Roadmap

Second-inspector claims with loser-pays fees; inspector deposits that can be slashed; supplier scorecards from passport history; real cross-chain payment through NEAR Intents on mainnet; a sub-account per passport holding its funds.

## Repository

```
contract/   Rust contract: src/lib.rs (NEAR bindings + unit tests), src/core.rs (pure logic + tests)
web/        index.html + app.js (live testnet app), demo.html (offline demo)
server/     relayer.mjs (gasless relayer + dry quote proxy), policy.mjs (+ tests)
scripts/    test-core.sh, testnet-demo.sh, serve.sh
```

## Disclosures

- **New project.** No existing product on another chain; everything here was written during the hackathon window.
- **Libraries:** near-sdk 5 (contract); @hot-labs/near-connect and near-api-js 5.1.1, loaded from esm.sh (web); near-api-js 5.1.1 (relayer); Google Fonts. No templates.
- **AI-generated code:** large parts of the contract, web app, relayer, tests and documentation were written with Claude (Anthropic) and reviewed, run and tested by the author.
- **Other projects by the author in this hackathon:** none.
- License: MIT ([LICENSE](LICENSE)).

   **Live app:** https://nauvalzuhdy.github.io/lot-passport/ · **Offline demo:** https://nauvalzuhdy.github.io/lot-passport/demo.html · **Contract:** nauval-lp-contract.testnet


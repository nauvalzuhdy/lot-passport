# Threat model

Who could end up with money or goods they should not have, and what stops them. "Contract" means `contract/src/lib.rs`; the rule names point to the code and tests.

## Parties

- **Buyer**: mints the passport and pays into it.
- **Supplier**: produces the goods and receives the payout.
- **Inspector**: agreed by both, the only account that can stamp the result.
- **Holder**: whoever holds the passport NFT after settlement. Holds the record, not money.
- **Operator**: deployer; can only mark company profiles as verified.
- **Relayer**: optional; pays gas for zero-deposit calls.

## Ways to keep money or goods, and what prevents them

| # | Attack | Who | Prevention | Where |
| --- | --- | --- | --- | --- |
| 1 | Buyer keeps the goods and gets the money back by claiming defects after delivery | Buyer | Inspection happens at the factory **before shipping**. A failed lot never ships; a passed lot is paid. The buyer has no method that moves money back after a pass. | `submit_inspection`, `fail_refunds_buyer_in_full` |
| 2 | Buyer stamps a fake failing result | Buyer | Only the inspector account recorded at mint can call `submit_inspection`. | `buyer_cannot_stamp` test |
| 3 | Inspector decides the outcome arbitrarily | Inspector | The inspector sends counts; the contract applies the ISO 2859-1 AQL 2.5 plan fixed at mint. `sampled` must equal the plan. The report hash is stored so the full report can be checked later. | `verdict`, `aql_plan` (core tests) |
| 4 | Pass rule changed after the deal | Anyone | Sample size and accept number are computed and stored at `create_lot`; no method changes them. | `mint_fixes_plan_and_names` |
| 5 | Supplier shows only good cartons | Supplier | Cartons are drawn by the contract from the block seed at the factory, once, after `mark_ready`. Packing must be done before the draw. | `draw_sample`, `pick_cartons` |
| 6 | Supplier pays without the buyer: wrong amount, wrong payer, second payment | Buyer, anyone | `ft_on_transfer` accepts only the configured token, only from the buyer, only the exact price, only once. Otherwise it panics and the token standard returns the funds. | `wrong_amount_is_refused` |
| 7 | Buyer withholds the last 10% forever | Buyer | After the 7-day window anyone can call `release`. | `anyone_can_release_after_window` |
| 8 | Someone else releases the retention early | Anyone | Before the window ends only the buyer can release. | `stranger_cannot_release_early` |
| 9 | Passport sold while money is still inside, so the money is stranded or follows the token | Holder | Transfer only when the lot is settled, nothing is held and no payout is pending. Roles (buyer, supplier, inspector) never move with the token. | `can_transfer`, `sale_during_claim_window_is_refused` |
| 10 | Payout fails (receiver not registered with the token) and money is silently lost | Supplier or buyer | `on_payout` marks the payment unsettled; `retry_payout` re-sends it, only to the recorded receiver. A pending payout blocks transfer. | `on_payout`, `retry_payout` |
| 11 | Unregistered or fake company | Anyone | All three parties must register; names are copied into the lot at mint. The operator can mark profiles verified after checking documents. Unverified profiles are shown as such. | `unregistered_supplier_is_refused` |
| 12 | Relayer abused to pay for token transfers or deposits | Anyone | Relayer sponsors only zero-deposit calls to this contract, only five methods, gas capped, short expiry. | `server/policy.mjs` + tests |
| 13 | Stolen inspector device key | Thief | The device holds a function-call key limited to this contract's gasless methods; it cannot move the inspector's NEAR or tokens. Revoke it with `near account delete-keys`. | `enableGasless` in `web/near.js` |
| 14 | Block producer biases the carton draw | Validator | Accepted risk for this use: biasing requires controlling block production at the exact moment and buys little. Mitigation path: commit-reveal with the inspector. | Limits section in README |
| 15 | Inspector colludes with supplier | Inspector + supplier | Not solvable on chain alone. Mitigations: inspector chosen by both, report hash on chain, public per-inspector history; roadmap: inspector deposit that can be slashed and second-inspector claims. | Roadmap |
| 16 | Supplier inflates its ranking with fake passing lots (buys from itself) | Supplier + inspector | Only inspected lots count, and a stamp needs a registered inspector that is a different account from buyer and supplier. Buyers see verified marks and units shipped. Roadmap: count only lots stamped by operator-verified inspectors and weight by paid volume. | `track_record`, `find_suppliers` |

## Out of scope for this build

Disputes after delivery for transit damage beyond the 10% retention; legal enforceability; privacy of trade data (everything on chain is public, registration numbers are hashed).

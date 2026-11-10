//! Lot Passport: every batch of goods traded between two companies is a
//! NEP-171 token that holds its own payment (a NEP-141 token such as USDC)
//! and its quality-inspection record.
//!
//! Flow: register_company (each party once) -> create_lot -> buyer pays with
//! ft_transfer_call -> mark_ready -> draw_sample -> submit_inspection ->
//! release -> nft_transfer.

mod core;

use crate::core::{
    aql_plan, can_release, can_transfer, carton_count, format_amount, page_range, passport_svg,
    pick_cartons, split_on_pass, svg_data_uri, valid_country, valid_hs_code, valid_incoterm,
    verdict, CardData, SamplingPlan, Status, CARTONS_TO_OPEN, CLAIM_WINDOW_NS,
};
use crate::core::{track_record, valid_hs_prefix, LotFacts};
use near_sdk::json_types::U128;
use near_sdk::serde_json::{self, json};
use near_sdk::store::IterableMap;
use near_sdk::{
    env, ext_contract, near, require, AccountId, Gas, NearToken, PanicOnDefault, Promise,
    PromiseError, PromiseOrValue, PromiseResult,
};

/// Storage deposit asked when minting a passport (covers the token's state).
const MINT_DEPOSIT: NearToken = NearToken::from_millinear(50);
/// Storage deposit asked when registering a company profile.
const COMPANY_DEPOSIT: NearToken = NearToken::from_millinear(20);
const GAS_FT_TRANSFER: Gas = Gas::from_tgas(15);
const GAS_CALLBACK: Gas = Gas::from_tgas(10);
const GAS_NFT_ON_TRANSFER: Gas = Gas::from_tgas(25);
const GAS_NFT_RESOLVE: Gas = Gas::from_tgas(10);

#[ext_contract(ext_ft)]
#[allow(dead_code)]
trait FungibleToken {
    fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>);
}

#[ext_contract(ext_nft_receiver)]
#[allow(dead_code)]
trait NonFungibleTokenReceiver {
    /// NEP-171: returns true if the token should be returned to the sender.
    fn nft_on_transfer(
        &mut self,
        sender_id: AccountId,
        previous_owner_id: AccountId,
        token_id: String,
        msg: String,
    ) -> bool;
}

#[ext_contract(ext_self)]
#[allow(dead_code)]
trait SelfCallbacks {
    fn on_payout(&mut self, token_id: String, to: AccountId, amount: U128, kind: String);
    fn nft_resolve_transfer(
        &mut self,
        owner_id: AccountId,
        receiver_id: AccountId,
        token_id: String,
    ) -> bool;
}

/// One money movement in a passport's life, for the history view and CSV export.
#[near(serializers = [borsh, json])]
#[derive(Clone)]
pub struct Payment {
    /// "funded", "paid_supplier", "refund_buyer", "release_retention".
    pub kind: String,
    pub from: AccountId,
    pub to: AccountId,
    pub amount: U128,
    pub at: u64,
    /// False while a payout that failed is waiting for retry_payout.
    pub settled: bool,
}

#[near(serializers = [borsh, json])]
#[derive(Clone)]
pub struct Holder {
    pub account_id: AccountId,
    pub since: u64,
}

/// A company's public profile. Public on chain: never put tax ids or
/// personal data here; a registration number can go in as a hash.
#[near(serializers = [borsh, json])]
#[derive(Clone)]
pub struct Company {
    pub account_id: AccountId,
    /// Legal name as on the company's registration, e.g. "PT Maju Teknik".
    pub legal_name: String,
    /// ISO 3166-1 alpha-2, e.g. "ID".
    pub country: String,
    pub city: String,
    /// Optional hash of the official registration number (USCC, HRB, KvK, ...).
    pub registration_hash: Option<String>,
    /// HS code prefixes the company says it makes or trades, e.g. ["8517", "8473"].
    /// Self-declared; the passport history is what proves it.
    pub hs_codes: Vec<String>,
    /// Set by the contract operator after checking the company's documents.
    pub verified: bool,
    pub registered_at: u64,
}

/// What is traded, in the terms customs and logistics already use.
#[near(serializers = [json])]
pub struct GoodsInput {
    pub description: String,
    /// Harmonized System code, 6 to 10 digits, e.g. "731815".
    pub hs_code: String,
    pub quantity: u64,
    pub units_per_carton: u32,
    /// ISO 3166-1 alpha-2 where the goods are made / shipped from.
    pub origin_country: String,
    /// ISO 3166-1 alpha-2 where the goods go.
    pub destination_country: String,
    /// Incoterms 2020 rule, e.g. "FOB".
    pub incoterm: String,
}

#[near(serializers = [borsh, json])]
#[derive(Clone)]
pub struct Lot {
    pub token_id: String,
    /// Who holds the passport now (NEP-171 owner). Starts as the buyer.
    pub owner_id: AccountId,
    pub buyer: AccountId,
    pub supplier: AccountId,
    /// The only account that may write the inspection result.
    pub inspector: AccountId,
    /// Legal names at the time of the deal, kept even if a profile changes later.
    pub buyer_name: String,
    pub supplier_name: String,
    pub inspector_name: String,
    pub product: String,
    pub hs_code: String,
    pub origin_country: String,
    pub destination_country: String,
    pub incoterm: String,
    pub quantity: u64,
    pub units_per_carton: u32,
    /// Price in the payment token's smallest unit.
    pub price: U128,
    pub sample_size: u32,
    pub accept_max: u32,
    pub status: Status,
    /// Money currently inside the passport.
    pub held: U128,
    pub paid_to_supplier: U128,
    pub refunded_to_buyer: U128,
    /// Cartons drawn by the contract; empty until draw_sample.
    pub cartons: Vec<u32>,
    pub defects: Option<u32>,
    /// Hash of the inspector's full report and photos, stored off chain.
    pub report_hash: Option<String>,
    pub inspected_at: Option<u64>,
    pub release_after: Option<u64>,
    pub created_at: u64,
    /// Every holder of the passport, oldest first. The last one is owner_id.
    pub holders: Vec<Holder>,
    /// Every money movement, in order.
    pub payments: Vec<Payment>,
}

/// A search result: a company and the record its passports prove.
#[near(serializers = [json])]
pub struct SupplierCard {
    pub company: Company,
    pub lots: u32,
    pub passed: u32,
    pub rejected: u32,
    pub in_progress: u32,
    pub units_passed: u64,
    pub defect_ppm: Option<u32>,
    /// Smoothed pass rate in basis points (5,000 = no history yet).
    pub score_bp: u32,
    pub traded_hs_codes: Vec<String>,
}

#[near(serializers = [json])]
pub struct NftContractMetadata {
    pub spec: String,
    pub name: String,
    pub symbol: String,
    pub icon: Option<String>,
    pub base_uri: Option<String>,
    pub reference: Option<String>,
    pub reference_hash: Option<String>,
}

#[near(serializers = [json])]
pub struct TokenMetadata {
    pub title: String,
    pub description: String,
    /// The passport card as an SVG data URI, drawn from live state, so
    /// wallets show every lot as a card whose stamp changes with its status.
    pub media: String,
    /// JSON string with the trade and inspection record.
    pub extra: String,
}

#[near(serializers = [json])]
pub struct JsonToken {
    pub token_id: String,
    pub owner_id: AccountId,
    pub metadata: TokenMetadata,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Contract {
    /// Operator who can mark companies as verified.
    operator: AccountId,
    /// The NEP-141 token this contract accepts as payment.
    token: AccountId,
    token_symbol: String,
    token_decimals: u8,
    companies: IterableMap<AccountId, Company>,
    lots: IterableMap<String, Lot>,
    next_id: u64,
}

#[near]
impl Contract {
    /// `token` e.g. "wrap.testnet" (wNEAR, 24 decimals) for testing, or a
    /// USDC contract (6 decimals).
    #[init]
    pub fn new(token: AccountId, token_symbol: String, token_decimals: u8) -> Self {
        Self {
            operator: env::predecessor_account_id(),
            token,
            token_symbol,
            token_decimals,
            companies: IterableMap::new(b"c"),
            lots: IterableMap::new(b"l"),
            next_id: 1,
        }
    }

    // ---------- companies ----------

    /// Each buyer, supplier and inspector registers once. Updating the
    /// profile clears the verified mark until the operator checks it again.
    #[payable]
    pub fn register_company(
        &mut self,
        legal_name: String,
        country: String,
        city: String,
        registration_hash: Option<String>,
        hs_codes: Option<Vec<String>>,
    ) {
        let account_id = env::predecessor_account_id();
        let first_time = !self.companies.contains_key(&account_id);
        if first_time {
            require!(
                env::attached_deposit() >= COMPANY_DEPOSIT,
                "Attach 0.02 NEAR to cover the profile's storage"
            );
        }
        require!(
            legal_name.trim().len() >= 2 && legal_name.len() <= 120,
            "Legal name must be 2 to 120 characters"
        );
        require!(
            valid_country(&country),
            "Country must be an ISO 3166-1 alpha-2 code such as ID or CN"
        );
        require!(city.len() <= 80, "City is too long");
        let hs_codes = hs_codes.unwrap_or_default();
        require!(hs_codes.len() <= 10, "At most 10 HS code prefixes");
        require!(
            hs_codes.iter().all(|h| valid_hs_prefix(h)),
            "HS code prefixes must be 4 to 10 digits, e.g. 8517"
        );
        let company = Company {
            account_id: account_id.clone(),
            legal_name: legal_name.trim().to_string(),
            country,
            city,
            registration_hash,
            hs_codes,
            verified: false,
            registered_at: env::block_timestamp(),
        };
        self.companies.insert(account_id.clone(), company);
        emit("company_registered", json!({ "account_id": account_id }));
    }

    pub fn verify_company(&mut self, account_id: AccountId) {
        require!(
            env::predecessor_account_id() == self.operator,
            "Only the operator can verify companies"
        );
        let mut c = self
            .companies
            .get(&account_id)
            .cloned()
            .unwrap_or_else(|| env::panic_str("Company not registered"));
        c.verified = true;
        self.companies.insert(account_id.clone(), c);
        emit("company_verified", json!({ "account_id": account_id }));
    }

    pub fn get_company(&self, account_id: AccountId) -> Option<Company> {
        self.companies.get(&account_id).cloned()
    }

    // ---------- 1. create_lot ----------

    /// Buyer mints the passport for an agreed deal. Both companies and the
    /// inspector must be registered. The sampling plan is computed and fixed
    /// now, so nobody can change the pass rule later.
    #[payable]
    pub fn create_lot(
        &mut self,
        supplier: AccountId,
        inspector: AccountId,
        goods: GoodsInput,
        price: U128,
    ) -> String {
        require!(
            env::attached_deposit() >= MINT_DEPOSIT,
            "Attach 0.05 NEAR to cover the passport's storage"
        );
        let buyer = env::predecessor_account_id();
        require!(
            buyer != supplier && buyer != inspector && supplier != inspector,
            "Buyer, supplier and inspector must be three different accounts"
        );
        let b = self.company_or_panic(&buyer, "Register your company first");
        let s = self.company_or_panic(&supplier, "The supplier has not registered its company");
        let i = self.company_or_panic(&inspector, "The inspector has not registered its company");

        require!(
            goods.description.trim().len() >= 2 && goods.description.len() <= 120,
            "Describe the goods in 2 to 120 characters"
        );
        require!(
            valid_hs_code(&goods.hs_code),
            "HS code must be 6 to 10 digits, e.g. 731815"
        );
        require!(
            valid_country(&goods.origin_country) && valid_country(&goods.destination_country),
            "Countries must be ISO 3166-1 alpha-2 codes"
        );
        require!(
            valid_incoterm(&goods.incoterm),
            "Incoterm must be one of EXW FCA FAS FOB CFR CIF CPT CIP DAP DPU DDP"
        );
        require!(
            goods.quantity > 0 && goods.units_per_carton > 0,
            "Quantity and units per carton must be positive"
        );
        require!(price.0 > 0, "Price must be positive");

        let token_id = format!("{:04}", self.next_id);
        self.next_id += 1;
        let SamplingPlan {
            sample_size,
            accept_max,
        } = aql_plan(goods.quantity);

        let lot = Lot {
            token_id: token_id.clone(),
            owner_id: buyer.clone(),
            buyer: buyer.clone(),
            supplier,
            inspector,
            buyer_name: b.legal_name,
            supplier_name: s.legal_name,
            inspector_name: i.legal_name,
            product: goods.description.trim().to_string(),
            hs_code: goods.hs_code,
            origin_country: goods.origin_country,
            destination_country: goods.destination_country,
            incoterm: goods.incoterm,
            quantity: goods.quantity,
            units_per_carton: goods.units_per_carton,
            price,
            sample_size,
            accept_max,
            status: Status::Created,
            held: U128(0),
            paid_to_supplier: U128(0),
            refunded_to_buyer: U128(0),
            cartons: vec![],
            defects: None,
            report_hash: None,
            inspected_at: None,
            release_after: None,
            created_at: env::block_timestamp(),
            holders: vec![Holder {
                account_id: buyer.clone(),
                since: env::block_timestamp(),
            }],
            payments: vec![],
        };
        self.lots.insert(token_id.clone(), lot);
        emit(
            "nft_mint",
            json!([{ "owner_id": buyer, "token_ids": [token_id] }]),
        );
        token_id
    }

    /// Payment arrives through the token: the buyer calls
    /// `ft_transfer_call(receiver_id = this contract, amount = price, msg = token_id)`.
    /// Any mismatch panics, and the token standard returns the money.
    pub fn ft_on_transfer(
        &mut self,
        sender_id: AccountId,
        amount: U128,
        msg: String,
    ) -> PromiseOrValue<U128> {
        require!(
            env::predecessor_account_id() == self.token,
            "Only the accepted token can fund a passport"
        );
        let mut lot = self.get(&msg);
        require!(
            lot.status == Status::Created,
            "This passport is already funded"
        );
        require!(
            sender_id == lot.buyer,
            "Only the buyer can fund this passport"
        );
        require!(amount == lot.price, "Amount must equal the agreed price");
        lot.held = amount;
        lot.status = Status::Funded;
        lot.payments.push(Payment {
            kind: "funded".into(),
            from: sender_id,
            to: env::current_account_id(),
            amount,
            at: env::block_timestamp(),
            settled: true,
        });
        self.lots.insert(msg.clone(), lot);
        emit("lot_funded", json!({ "token_id": msg, "amount": amount }));
        PromiseOrValue::Value(U128(0))
    }

    /// Supplier: production is done, the goods wait at the factory.
    pub fn mark_ready(&mut self, token_id: String) {
        let mut lot = self.get(&token_id);
        require!(
            env::predecessor_account_id() == lot.supplier,
            "Only the supplier can mark the lot ready"
        );
        require!(
            lot.status == Status::Funded,
            "The passport must be funded first"
        );
        lot.status = Status::ReadyForInspection;
        self.lots.insert(token_id.clone(), lot);
        emit("lot_ready", json!({ "token_id": token_id }));
    }

    // ---------- 2. draw_sample ----------

    /// Draws which cartons the inspector must open, from the block's random
    /// seed. Called once, at the factory, after the goods are ready, so the
    /// supplier cannot prepare the chosen cartons in advance.
    pub fn draw_sample(&mut self, token_id: String) -> Vec<u32> {
        let mut lot = self.get(&token_id);
        let caller = env::predecessor_account_id();
        require!(
            caller == lot.inspector || caller == lot.supplier,
            "Only the inspector or supplier can start the draw"
        );
        require!(
            lot.status == Status::ReadyForInspection,
            "The lot is not ready for inspection"
        );
        require!(
            lot.cartons.is_empty(),
            "Cartons were already drawn for this lot"
        );
        let total = carton_count(lot.quantity, lot.units_per_carton);
        lot.cartons = pick_cartons(&env::random_seed(), total, CARTONS_TO_OPEN);
        let cartons = lot.cartons.clone();
        self.lots.insert(token_id.clone(), lot);
        emit(
            "sample_drawn",
            json!({ "token_id": token_id, "cartons": cartons, "of_total": total }),
        );
        cartons
    }

    // ---------- 3. submit_inspection ----------

    /// Inspector writes the counts. The contract applies the AQL plan and
    /// moves the money: pass pays the supplier 90% and keeps 10% for the
    /// claim window; fail refunds the buyer in full and the goods never ship.
    pub fn submit_inspection(
        &mut self,
        token_id: String,
        sampled: u32,
        defects: u32,
        report_hash: String,
    ) -> Promise {
        let mut lot = self.get(&token_id);
        require!(
            env::predecessor_account_id() == lot.inspector,
            "Only the agreed inspector can stamp this passport"
        );
        require!(
            lot.status == Status::ReadyForInspection,
            "The lot is not waiting for inspection"
        );
        require!(
            !lot.cartons.is_empty(),
            "Draw the cartons before inspecting"
        );
        require!(!report_hash.is_empty(), "A report hash is required");
        let plan = SamplingPlan {
            sample_size: lot.sample_size,
            accept_max: lot.accept_max,
        };
        let passed = verdict(plan, sampled, defects).unwrap_or_else(|e| env::panic_str(e));

        let now = env::block_timestamp();
        lot.defects = Some(defects);
        lot.report_hash = Some(report_hash);
        lot.inspected_at = Some(now);

        let promise = if passed {
            let (pay_now, keep) = split_on_pass(lot.held.0);
            lot.held = U128(keep);
            lot.paid_to_supplier = U128(lot.paid_to_supplier.0 + pay_now);
            lot.release_after = Some(now + CLAIM_WINDOW_NS);
            lot.status = Status::Passed;
            let to = lot.supplier.clone();
            lot.payments.push(Payment {
                kind: "paid_supplier".into(),
                from: env::current_account_id(),
                to: to.clone(),
                amount: U128(pay_now),
                at: now,
                settled: true,
            });
            self.pay(to, pay_now, &token_id, "paid_supplier")
        } else {
            let back = lot.held.0;
            lot.held = U128(0);
            lot.refunded_to_buyer = U128(back);
            lot.status = Status::Rejected;
            let to = lot.buyer.clone();
            lot.payments.push(Payment {
                kind: "refund_buyer".into(),
                from: env::current_account_id(),
                to: to.clone(),
                amount: U128(back),
                at: now,
                settled: true,
            });
            self.pay(to, back, &token_id, "refund_buyer")
        };
        emit(
            "lot_inspected",
            json!({ "token_id": token_id, "sampled": sampled, "defects": defects, "accept_max": lot.accept_max, "passed": passed }),
        );
        self.lots.insert(token_id, lot);
        promise
    }

    // ---------- 4. release ----------

    /// Pays out the 10% retention: the buyer may release it early, anyone
    /// may release it once the claim window has ended.
    pub fn release(&mut self, token_id: String) -> Promise {
        let mut lot = self.get(&token_id);
        require!(
            lot.status == Status::Passed,
            "Only a passed lot has money to release"
        );
        let caller_is_buyer = env::predecessor_account_id() == lot.buyer;
        require!(
            can_release(
                env::block_timestamp(),
                lot.release_after.unwrap_or(u64::MAX),
                caller_is_buyer
            ),
            "The claim window is still open; only the buyer can release early"
        );
        let amount = lot.held.0;
        lot.held = U128(0);
        lot.paid_to_supplier = U128(lot.paid_to_supplier.0 + amount);
        lot.status = Status::Completed;
        let supplier = lot.supplier.clone();
        lot.payments.push(Payment {
            kind: "release_retention".into(),
            from: env::current_account_id(),
            to: supplier.clone(),
            amount: U128(amount),
            at: env::block_timestamp(),
            settled: true,
        });
        self.lots.insert(token_id.clone(), lot);
        emit(
            "lot_completed",
            json!({ "token_id": token_id, "released": U128(amount) }),
        );
        self.pay(supplier, amount, &token_id, "release_retention")
    }

    /// Callback after every payout. If the token transfer failed (for example
    /// the receiver was not registered with the token), the payment is marked
    /// unsettled so the money is not lost and anyone can retry it.
    #[private]
    pub fn on_payout(&mut self, token_id: String, to: AccountId, amount: U128, kind: String) {
        // ft_transfer returns no value, so read the raw result instead of deserializing one.
        if matches!(env::promise_result(0), PromiseResult::Successful(_)) {
            return;
        }
        let mut lot = self.get(&token_id);
        if let Some(p) = lot
            .payments
            .iter_mut()
            .rev()
            .find(|p| p.kind == kind && p.to == to && p.amount == amount && p.settled)
        {
            p.settled = false;
        }
        self.lots.insert(token_id.clone(), lot);
        emit(
            "payout_failed",
            json!({ "token_id": token_id, "to": to, "amount": amount, "kind": kind }),
        );
    }

    /// Re-sends every unsettled payout of a passport. Anyone may call it,
    /// money only ever goes to the recorded receiver.
    pub fn retry_payout(&mut self, token_id: String) -> Promise {
        let mut lot = self.get(&token_id);
        let idx = lot
            .payments
            .iter()
            .position(|p| !p.settled)
            .unwrap_or_else(|| env::panic_str("No failed payout to retry"));
        lot.payments[idx].settled = true;
        let p = lot.payments[idx].clone();
        self.lots.insert(token_id.clone(), lot);
        self.pay(p.to, p.amount.0, &token_id, &p.kind)
    }

    // ---------- 5. transfer (NEP-171 nft_transfer) ----------

    /// Hands the passport, with its whole record, to a new owner, for
    /// example the buyer's own customer. Only after the lot is settled and
    /// nothing is left inside. The holder owns the record; the buyer,
    /// supplier and inspector roles of the deal never move.
    #[payable]
    pub fn nft_transfer(
        &mut self,
        receiver_id: AccountId,
        token_id: String,
        approval_id: Option<u64>,
        memo: Option<String>,
    ) {
        require!(
            env::attached_deposit() == NearToken::from_yoctonear(1),
            "Attach exactly 1 yoctoNEAR"
        );
        require!(approval_id.is_none(), "Approvals are not supported yet");
        let sender = env::predecessor_account_id();
        self.internal_transfer(&sender, &receiver_id, &token_id, memo);
    }

    /// NEP-171 transfer to a contract that is told about it and may give it back.
    #[payable]
    pub fn nft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        token_id: String,
        approval_id: Option<u64>,
        memo: Option<String>,
        msg: String,
    ) -> PromiseOrValue<bool> {
        require!(
            env::attached_deposit() == NearToken::from_yoctonear(1),
            "Attach exactly 1 yoctoNEAR"
        );
        require!(approval_id.is_none(), "Approvals are not supported yet");
        let sender = env::predecessor_account_id();
        self.internal_transfer(&sender, &receiver_id, &token_id, memo);
        ext_nft_receiver::ext(receiver_id.clone())
            .with_static_gas(GAS_NFT_ON_TRANSFER)
            .nft_on_transfer(sender.clone(), sender.clone(), token_id.clone(), msg)
            .then(
                ext_self::ext(env::current_account_id())
                    .with_static_gas(GAS_NFT_RESOLVE)
                    .nft_resolve_transfer(sender, receiver_id, token_id),
            )
            .into()
    }

    /// Returns true if the token was kept by the receiver.
    #[private]
    pub fn nft_resolve_transfer(
        &mut self,
        #[callback_result] must_return: Result<bool, PromiseError>,
        owner_id: AccountId,
        receiver_id: AccountId,
        token_id: String,
    ) -> bool {
        // Keep the token unless the receiver explicitly asked to return it.
        let give_back = matches!(must_return, Ok(true));
        if !give_back {
            return true;
        }
        let mut lot = self.get(&token_id);
        if lot.owner_id != receiver_id {
            return true; // receiver already moved it on
        }
        lot.owner_id = owner_id.clone();
        lot.holders.push(Holder {
            account_id: owner_id.clone(),
            since: env::block_timestamp(),
        });
        self.lots.insert(token_id.clone(), lot);
        emit(
            "nft_transfer",
            json!([{ "old_owner_id": receiver_id, "new_owner_id": owner_id, "token_ids": [token_id] }]),
        );
        false
    }

    // ---------- views ----------

    pub fn get_lot(&self, token_id: String) -> Option<Lot> {
        self.lots.get(&token_id).cloned()
    }

    /// Every lot where the account is buyer, supplier, inspector or holder.
    pub fn lots_for_account(&self, account_id: AccountId) -> Vec<Lot> {
        self.lots
            .values()
            .filter(|l| {
                l.buyer == account_id
                    || l.supplier == account_id
                    || l.inspector == account_id
                    || l.owner_id == account_id
            })
            .cloned()
            .collect()
    }

    /// Finds suppliers by what they make and where they are, ranked by the
    /// track record their passports prove (inspected lots only).
    /// `hs_prefix` matches declared HS codes or the HS code of any past lot.
    pub fn find_suppliers(
        &self,
        hs_prefix: Option<String>,
        country: Option<String>,
        limit: Option<u64>,
    ) -> Vec<SupplierCard> {
        let mut out: Vec<SupplierCard> = self
            .companies
            .values()
            .filter(|c| country.as_ref().map_or(true, |k| &c.country == k))
            .filter_map(|c| {
                let lots: Vec<&Lot> = self
                    .lots
                    .values()
                    .filter(|l| l.supplier == c.account_id)
                    .collect();
                let matching: Vec<&Lot> = match &hs_prefix {
                    Some(p) => lots
                        .iter()
                        .copied()
                        .filter(|l| l.hs_code.starts_with(p.as_str()))
                        .collect(),
                    None => lots.clone(),
                };
                let declared = hs_prefix.as_ref().map_or(true, |p| {
                    c.hs_codes
                        .iter()
                        .any(|h| h.starts_with(p.as_str()) || p.starts_with(h.as_str()))
                });
                if !declared && matching.is_empty() {
                    return None;
                }
                let facts: Vec<LotFacts> = matching
                    .iter()
                    .map(|l| LotFacts {
                        status: l.status,
                        quantity: l.quantity,
                        sample_size: l.sample_size,
                        defects: l.defects,
                    })
                    .collect();
                let r = track_record(&facts);
                let mut traded: Vec<String> = matching.iter().map(|l| l.hs_code.clone()).collect();
                traded.sort();
                traded.dedup();
                Some(SupplierCard {
                    company: c.clone(),
                    lots: r.lots,
                    passed: r.passed,
                    rejected: r.rejected,
                    in_progress: r.in_progress,
                    units_passed: r.units_passed,
                    defect_ppm: r.defect_ppm,
                    score_bp: r.score_bp,
                    traded_hs_codes: traded,
                })
            })
            .collect();
        out.sort_by(|a, b| b.score_bp.cmp(&a.score_bp).then(b.passed.cmp(&a.passed)));
        out.truncate(limit.unwrap_or(20).min(50) as usize);
        out
    }

    /// Every passport a supplier has ever had: its public quality record.
    pub fn lots_for_supplier(&self, account_id: AccountId) -> Vec<Lot> {
        self.lots
            .values()
            .filter(|l| l.supplier == account_id)
            .cloned()
            .collect()
    }

    pub fn nft_token(&self, token_id: String) -> Option<JsonToken> {
        self.lots.get(&token_id).map(|l| self.to_json_token(l))
    }

    // NEP-181 enumeration, so wallets can list the passports.
    pub fn nft_total_supply(&self) -> U128 {
        U128(self.lots.len() as u128)
    }

    pub fn nft_tokens(&self, from_index: Option<U128>, limit: Option<u64>) -> Vec<JsonToken> {
        let (start, end) = page_range(self.lots.len() as usize, from_index.map(|x| x.0), limit);
        self.lots
            .values()
            .skip(start)
            .take(end - start)
            .map(|l| self.to_json_token(l))
            .collect()
    }

    pub fn nft_supply_for_owner(&self, account_id: AccountId) -> U128 {
        U128(
            self.lots
                .values()
                .filter(|l| l.owner_id == account_id)
                .count() as u128,
        )
    }

    pub fn nft_tokens_for_owner(
        &self,
        account_id: AccountId,
        from_index: Option<U128>,
        limit: Option<u64>,
    ) -> Vec<JsonToken> {
        let owned: Vec<&Lot> = self
            .lots
            .values()
            .filter(|l| l.owner_id == account_id)
            .collect();
        let (start, end) = page_range(owned.len(), from_index.map(|x| x.0), limit);
        owned[start..end]
            .iter()
            .map(|l| self.to_json_token(l))
            .collect()
    }

    pub fn nft_metadata(&self) -> NftContractMetadata {
        NftContractMetadata {
            spec: "nft-1.0.0".into(),
            name: "Lot Passport".into(),
            symbol: "LOTP".into(),
            icon: None,
            base_uri: None,
            reference: None,
            reference_hash: None,
        }
    }

    pub fn payment_token(&self) -> serde_json::Value {
        json!({ "account_id": self.token, "symbol": self.token_symbol, "decimals": self.token_decimals })
    }
}

impl Contract {
    fn get(&self, token_id: &str) -> Lot {
        self.lots
            .get(token_id)
            .cloned()
            .unwrap_or_else(|| env::panic_str("No passport with this id"))
    }

    fn company_or_panic(&self, account_id: &AccountId, msg: &str) -> Company {
        self.companies
            .get(account_id)
            .cloned()
            .unwrap_or_else(|| env::panic_str(msg))
    }

    /// Sends the payment token to a receiver, then checks the result in
    /// on_payout. The receiver must be registered with the token contract.
    fn pay(&self, to: AccountId, amount: u128, token_id: &str, kind: &str) -> Promise {
        ext_ft::ext(self.token.clone())
            .with_attached_deposit(NearToken::from_yoctonear(1))
            .with_static_gas(GAS_FT_TRANSFER)
            .ft_transfer(
                to.clone(),
                U128(amount),
                Some(format!("Lot Passport #{token_id}")),
            )
            .then(
                ext_self::ext(env::current_account_id())
                    .with_static_gas(GAS_CALLBACK)
                    .on_payout(token_id.to_string(), to, U128(amount), kind.to_string()),
            )
    }

    fn internal_transfer(
        &mut self,
        sender: &AccountId,
        receiver_id: &AccountId,
        token_id: &str,
        memo: Option<String>,
    ) {
        let mut lot = self.get(token_id);
        require!(
            *sender == lot.owner_id,
            "Only the passport's owner can transfer it"
        );
        require!(sender != receiver_id, "Cannot transfer to yourself");
        let pending = lot.payments.iter().filter(|p| !p.settled).count();
        require!(
            can_transfer(lot.status, lot.held.0, pending),
            "A passport can change hands only after its lot is settled and nothing is left inside"
        );
        lot.owner_id = receiver_id.clone();
        lot.holders.push(Holder {
            account_id: receiver_id.clone(),
            since: env::block_timestamp(),
        });
        self.lots.insert(token_id.to_string(), lot);
        emit(
            "nft_transfer",
            json!([{ "old_owner_id": sender, "new_owner_id": receiver_id, "token_ids": [token_id], "memo": memo }]),
        );
    }

    fn to_json_token(&self, l: &Lot) -> JsonToken {
        let extra = json!({
            "status": l.status, "buyer": l.buyer_name, "supplier": l.supplier_name, "inspector": l.inspector_name,
            "hs_code": l.hs_code, "origin": l.origin_country, "destination": l.destination_country, "incoterm": l.incoterm,
            "quantity": l.quantity, "sample_size": l.sample_size, "accept_max": l.accept_max, "cartons": l.cartons,
            "defects": l.defects, "report_hash": l.report_hash, "held": l.held, "paid_to_supplier": l.paid_to_supplier,
            "refunded_to_buyer": l.refunded_to_buyer, "token": self.token_symbol
        });
        let media = svg_data_uri(&passport_svg(&CardData {
            token_id: &l.token_id,
            product: &l.product,
            quantity: l.quantity,
            hs_code: &l.hs_code,
            incoterm: &l.incoterm,
            origin: &l.origin_country,
            destination: &l.destination_country,
            supplier_name: &l.supplier_name,
            buyer_name: &l.buyer_name,
            inspector_name: &l.inspector_name,
            status: l.status,
            held: l.held.0,
            symbol: &self.token_symbol,
            decimals: self.token_decimals,
            sample_size: l.sample_size,
            accept_max: l.accept_max,
            defects: l.defects,
        }));
        JsonToken {
            token_id: l.token_id.clone(),
            owner_id: l.owner_id.clone(),
            metadata: TokenMetadata {
                title: format!("Lot Passport #{} · {}", l.token_id, l.product),
                description: format!(
                    "{} pcs (HS {}) from {} ({}) to {} ({}), {}. Price {} {}. ISO 2859-1 AQL 2.5: pass if ≤ {} of {} defective.",
                    l.quantity, l.hs_code, l.supplier_name, l.origin_country, l.buyer_name, l.destination_country, l.incoterm,
                    format_amount(l.price.0, self.token_decimals), self.token_symbol, l.accept_max, l.sample_size
                ),
                media,
                extra: extra.to_string(),
            },
        }
    }
}

/// NEP-297 event log: nft_mint and nft_transfer follow NEP-171; the
/// other events are this contract's own.
fn emit(event: &str, data: serde_json::Value) {
    let standard = if event.starts_with("nft_") {
        "nep171"
    } else {
        "lot_passport"
    };
    let data = if data.is_array() { data } else { json!([data]) };
    let log = json!({ "standard": standard, "version": "1.0.0", "event": event, "data": data });
    env::log_str(&format!("EVENT_JSON:{log}"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use near_sdk::test_utils::VMContextBuilder;
    use near_sdk::testing_env;

    const PRICE: u128 = 1_536_000_000_000; // $1,536,000.00 in USDC units

    fn acc(s: &str) -> AccountId {
        s.parse().unwrap()
    }

    fn as_(who: &str, deposit: NearToken, ts: u64) {
        let mut b = VMContextBuilder::new();
        b.current_account_id(acc("lot-passport.testnet"))
            .predecessor_account_id(acc(who))
            .signer_account_id(acc(who))
            .attached_deposit(deposit)
            .block_timestamp(ts)
            .random_seed([7u8; 32]);
        testing_env!(b.build());
    }

    fn setup() -> Contract {
        as_("operator.testnet", NearToken::from_yoctonear(0), 1);
        let mut c = Contract::new(acc("usdc.testnet"), "USDC".into(), 6);
        for (who, name, country) in [
            ("buyer.testnet", "Nordlicht Rechenzentren GmbH", "DE"),
            (
                "supplier.testnet",
                "Shenzhen Lightwave Optics Co., Ltd.",
                "CN",
            ),
            ("inspector.testnet", "V-Trust Inspection", "CN"),
        ] {
            as_(who, COMPANY_DEPOSIT, 2);
            c.register_company(
                name.into(),
                country.into(),
                "City".into(),
                None,
                Some(vec!["8517".into()]),
            );
        }
        c
    }

    fn goods() -> GoodsInput {
        GoodsInput {
            description: "800G OSFP optical transceivers".into(),
            hs_code: "851762".into(),
            quantity: 2_400,
            units_per_carton: 20,
            origin_country: "CN".into(),
            destination_country: "DE".into(),
            incoterm: "FOB".into(),
        }
    }

    /// Mints, funds, marks ready and draws: returns the lot id.
    fn ready_lot(c: &mut Contract) -> String {
        as_("buyer.testnet", MINT_DEPOSIT, 3);
        let id = c.create_lot(
            acc("supplier.testnet"),
            acc("inspector.testnet"),
            goods(),
            U128(PRICE),
        );
        as_("usdc.testnet", NearToken::from_yoctonear(0), 4);
        let _ = c.ft_on_transfer(acc("buyer.testnet"), U128(PRICE), id.clone());
        as_("supplier.testnet", NearToken::from_yoctonear(0), 5);
        c.mark_ready(id.clone());
        as_("inspector.testnet", NearToken::from_yoctonear(0), 6);
        let cartons = c.draw_sample(id.clone());
        assert_eq!(cartons.len(), 6);
        id
    }

    #[test]
    fn mint_fixes_plan_and_names() {
        let mut c = setup();
        as_("buyer.testnet", MINT_DEPOSIT, 3);
        let id = c.create_lot(
            acc("supplier.testnet"),
            acc("inspector.testnet"),
            goods(),
            U128(PRICE),
        );
        let l = c.get_lot(id).unwrap();
        assert_eq!((l.sample_size, l.accept_max), (125, 7));
        assert_eq!(l.supplier_name, "Shenzhen Lightwave Optics Co., Ltd.");
        assert_eq!(l.holders.len(), 1);
        assert_eq!(l.status, Status::Created);
    }

    #[test]
    #[should_panic(expected = "The supplier has not registered its company")]
    fn unregistered_supplier_is_refused() {
        let mut c = setup();
        as_("buyer.testnet", MINT_DEPOSIT, 3);
        c.create_lot(
            acc("stranger.testnet"),
            acc("inspector.testnet"),
            goods(),
            U128(PRICE),
        );
    }

    #[test]
    #[should_panic(expected = "Amount must equal the agreed price")]
    fn wrong_amount_is_refused() {
        let mut c = setup();
        as_("buyer.testnet", MINT_DEPOSIT, 3);
        let id = c.create_lot(
            acc("supplier.testnet"),
            acc("inspector.testnet"),
            goods(),
            U128(PRICE),
        );
        as_("usdc.testnet", NearToken::from_yoctonear(0), 4);
        let _ = c.ft_on_transfer(acc("buyer.testnet"), U128(PRICE - 1), id);
    }

    #[test]
    #[should_panic(expected = "Only the agreed inspector can stamp this passport")]
    fn buyer_cannot_stamp() {
        let mut c = setup();
        let id = ready_lot(&mut c);
        as_("buyer.testnet", NearToken::from_yoctonear(0), 7);
        c.submit_inspection(id, 125, 0, "sha256:x".into());
    }

    #[test]
    fn pass_pays_90_keeps_10_then_release_and_transfer() {
        let mut c = setup();
        let id = ready_lot(&mut c);
        as_("inspector.testnet", NearToken::from_yoctonear(0), 7);
        c.submit_inspection(id.clone(), 125, 2, "sha256:report".into());
        let l = c.get_lot(id.clone()).unwrap();
        assert_eq!(l.status, Status::Passed);
        assert_eq!(l.held.0, PRICE / 10);
        assert_eq!(l.paid_to_supplier.0, PRICE - PRICE / 10);

        as_("buyer.testnet", NearToken::from_yoctonear(0), 8);
        c.release(id.clone());
        let l = c.get_lot(id.clone()).unwrap();
        assert_eq!(l.status, Status::Completed);
        assert_eq!(l.held.0, 0);
        assert_eq!(l.payments.len(), 3);

        as_("buyer.testnet", NearToken::from_yoctonear(1), 9);
        c.nft_transfer(acc("customer.testnet"), id.clone(), None, None);
        let l = c.get_lot(id).unwrap();
        assert_eq!(l.owner_id, acc("customer.testnet"));
        assert_eq!(l.holders.len(), 2);
        assert_eq!(l.buyer, acc("buyer.testnet")); // roles never move
    }

    #[test]
    #[should_panic(expected = "only after its lot is settled")]
    fn sale_during_claim_window_is_refused() {
        let mut c = setup();
        let id = ready_lot(&mut c);
        as_("inspector.testnet", NearToken::from_yoctonear(0), 7);
        c.submit_inspection(id.clone(), 125, 2, "sha256:report".into());
        as_("buyer.testnet", NearToken::from_yoctonear(1), 8);
        c.nft_transfer(acc("customer.testnet"), id, None, None);
    }

    #[test]
    fn fail_refunds_buyer_in_full() {
        let mut c = setup();
        let id = ready_lot(&mut c);
        as_("inspector.testnet", NearToken::from_yoctonear(0), 7);
        c.submit_inspection(id.clone(), 125, 8, "sha256:report".into());
        let l = c.get_lot(id).unwrap();
        assert_eq!(l.status, Status::Rejected);
        assert_eq!(l.refunded_to_buyer.0, PRICE);
        assert_eq!(l.held.0, 0);
    }

    #[test]
    #[should_panic(expected = "The claim window is still open")]
    fn stranger_cannot_release_early() {
        let mut c = setup();
        let id = ready_lot(&mut c);
        as_("inspector.testnet", NearToken::from_yoctonear(0), 7);
        c.submit_inspection(id.clone(), 125, 0, "sha256:report".into());
        as_("stranger.testnet", NearToken::from_yoctonear(0), 8);
        c.release(id);
    }

    #[test]
    fn anyone_can_release_after_window() {
        let mut c = setup();
        let id = ready_lot(&mut c);
        as_("inspector.testnet", NearToken::from_yoctonear(0), 7);
        c.submit_inspection(id.clone(), 125, 0, "sha256:report".into());
        as_(
            "stranger.testnet",
            NearToken::from_yoctonear(0),
            7 + CLAIM_WINDOW_NS,
        );
        c.release(id.clone());
        assert_eq!(c.get_lot(id).unwrap().status, Status::Completed);
    }

    #[test]
    fn find_suppliers_ranks_by_passport_record() {
        let mut c = setup();
        // A second supplier with no history.
        as_("newcomer.testnet", COMPANY_DEPOSIT, 2);
        c.register_company(
            "Suzhou Newcomer Photonics".into(),
            "CN".into(),
            "Suzhou".into(),
            None,
            Some(vec!["851762".into()]),
        );
        let id = ready_lot(&mut c);
        as_("inspector.testnet", NearToken::from_yoctonear(0), 7);
        c.submit_inspection(id, 125, 2, "sha256:report".into());

        let found = c.find_suppliers(Some("851762".into()), Some("CN".into()), None);
        let names: Vec<&str> = found
            .iter()
            .map(|s| s.company.legal_name.as_str())
            .collect();
        assert_eq!(names[0], "Shenzhen Lightwave Optics Co., Ltd."); // 1 pass beats no history
        assert!(names.contains(&"Suzhou Newcomer Photonics"));
        assert_eq!(found[0].passed, 1);
        assert_eq!(found[0].traded_hs_codes, vec!["851762".to_string()]);
        // The buyer in Germany declared 8517 too, but has no passed lots as a supplier.
        assert!(c
            .find_suppliers(Some("8517".into()), Some("DE".into()), None)
            .iter()
            .all(|s| s.passed == 0));
        // An unrelated HS code finds nobody.
        assert!(c.find_suppliers(Some("6403".into()), None, None).is_empty());
    }

    #[test]
    fn enumeration_and_card_image() {
        let mut c = setup();
        let id = ready_lot(&mut c);
        assert_eq!(c.nft_total_supply().0, 1);
        assert_eq!(c.nft_supply_for_owner(acc("buyer.testnet")).0, 1);
        let t = c.nft_tokens_for_owner(acc("buyer.testnet"), None, Some(10));
        assert_eq!(t.len(), 1);
        assert!(t[0].metadata.media.starts_with("data:image/svg+xml"));
        assert_eq!(c.nft_token(id).unwrap().owner_id, acc("buyer.testnet"));
    }
}

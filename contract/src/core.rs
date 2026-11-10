//! Pure logic of a Lot Passport: AQL sampling plan, random carton draw,
//! verdict, payout split and the status machine. No NEAR dependency, so it
//! can be unit-tested with plain `rustc --test` and reused off-chain.

#[cfg(not(standalone))]
use near_sdk::near;

/// Share of the price kept inside the passport during the claim window, in percent.
pub const RETENTION_PCT: u128 = 10;
/// Claim window after a passed inspection: 7 days in nanoseconds.
pub const CLAIM_WINDOW_NS: u64 = 7 * 24 * 60 * 60 * 1_000_000_000;
/// How many cartons the contract asks the inspector to open.
pub const CARTONS_TO_OPEN: u32 = 6;

#[cfg_attr(not(standalone), near(serializers = [borsh, json]))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Passport minted, waiting for the buyer's payment.
    Created,
    /// Payment locked inside the passport; supplier is producing.
    Funded,
    /// Supplier says the goods are ready; inspection can start.
    ReadyForInspection,
    /// Inspection passed: 90% paid to supplier, 10% held for claims.
    Passed,
    /// Inspection failed: goods never ship, buyer refunded in full.
    Rejected,
    /// Claim window over or buyer confirmed: everything paid out.
    Completed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SamplingPlan {
    /// Pieces the inspector must check.
    pub sample_size: u32,
    /// Lot passes if problem pieces are at most this number.
    pub accept_max: u32,
}

/// ISO 2859-1, general inspection level II, single sampling, normal
/// inspection, AQL 2.5. Sample size code letter by lot size, then the
/// accept number for AQL 2.5 (table arrows resolved to the nearest plan).
pub fn aql_plan(lot_size: u64) -> SamplingPlan {
    // (max lot size, code letter sample size)
    const CODE: [(u64, u32); 15] = [
        (8, 2),
        (15, 3),
        (25, 5),
        (50, 8),
        (90, 13),
        (150, 20),
        (280, 32),
        (500, 50),
        (1_200, 80),
        (3_200, 125),
        (10_000, 200),
        (35_000, 315),
        (150_000, 500),
        (500_000, 800),
        (u64::MAX, 1_250),
    ];
    let n = CODE
        .iter()
        .find(|(max, _)| lot_size <= *max)
        .map(|(_, n)| *n)
        .unwrap_or(1_250);
    // Arrows in the AQL 2.5 column: n = 2, 3, 8 use the n = 5 plan; n >= 800 use n = 500.
    let effective = match n {
        0..=8 => 5,
        800.. => 500,
        other => other,
    };
    let accept_max = match effective {
        5 => 0,
        13 | 20 => 1,
        32 => 2,
        50 => 3,
        80 => 5,
        125 => 7,
        200 => 10,
        315 => 14,
        _ => 21, // 500
    };
    let sample_size = if (effective as u64) > lot_size {
        lot_size as u32
    } else {
        effective
    };
    SamplingPlan {
        sample_size,
        accept_max,
    }
}

/// Number of cartons in a lot, rounded up.
pub fn carton_count(quantity: u64, units_per_carton: u32) -> u32 {
    assert!(units_per_carton > 0, "units_per_carton must be positive");
    let upc = units_per_carton as u64;
    ((quantity + upc - 1) / upc) as u32
}

/// Picks `k` distinct carton numbers in 1..=total from a 32-byte seed
/// (on chain: `env::random_seed()`). Same seed gives the same cartons, so
/// anyone can re-run the draw from the recorded block.
pub fn pick_cartons(seed: &[u8], total: u32, k: u32) -> Vec<u32> {
    let k = k.min(total);
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    for (i, b) in seed.iter().enumerate() {
        state ^= (*b as u64) << ((i % 8) * 8);
        state = state.rotate_left(7).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    }
    if state == 0 {
        state = 1;
    }
    let mut out: Vec<u32> = Vec::with_capacity(k as usize);
    while (out.len() as u32) < k {
        // xorshift64*
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        let r = state.wrapping_mul(0x2545_F491_4F6C_DD1D);
        let carton = (r % total as u64) as u32 + 1;
        if !out.contains(&carton) {
            out.push(carton);
        }
    }
    out.sort_unstable();
    out
}

/// The contract, not the inspector, decides the verdict from the counts.
pub fn verdict(plan: SamplingPlan, sampled: u32, defects: u32) -> Result<bool, &'static str> {
    if sampled != plan.sample_size {
        return Err("Sampled pieces must equal the sampling plan");
    }
    if defects > sampled {
        return Err("Defects cannot exceed sampled pieces");
    }
    Ok(defects <= plan.accept_max)
}

/// On a pass: (paid now to supplier, kept for the claim window).
pub fn split_on_pass(price: u128) -> (u128, u128) {
    let kept = price * RETENTION_PCT / 100;
    (price - kept, kept)
}

/// The retention can be released by the buyer at any time after a pass,
/// or by anyone once the claim window has ended.
pub fn can_release(now_ns: u64, release_after_ns: u64, caller_is_buyer: bool) -> bool {
    caller_is_buyer || now_ns >= release_after_ns
}

/// A passport may change hands only once its lot is settled and no money
/// is left inside it or waiting to be paid out, so no money can travel with
/// the token or be stranded by a sale.
pub fn can_transfer(status: Status, held: u128, pending_payouts: usize) -> bool {
    matches!(status, Status::Completed | Status::Rejected) && held == 0 && pending_payouts == 0
}

/// NEP-181 style pagination over `len` items: returns the index range.
pub fn page_range(len: usize, from_index: Option<u128>, limit: Option<u64>) -> (usize, usize) {
    let start = from_index.unwrap_or(0).min(len as u128) as usize;
    let limit = limit.unwrap_or(50).min(100) as usize;
    (start, (start + limit).min(len))
}

/// Formats a token amount with `decimals` as "4,278.00" (rounded to 2 places).
pub fn format_amount(amount: u128, decimals: u8) -> String {
    let cents = if decimals >= 2 {
        let unit = 10u128.pow((decimals - 2) as u32);
        (amount + unit / 2) / unit
    } else {
        amount * 10u128.pow((2 - decimals) as u32)
    };
    let (whole, frac) = (cents / 100, cents % 100);
    let digits = whole.to_string();
    let mut grouped = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    format!("{grouped}.{frac:02}")
}

/// ISO 3166-1 alpha-2 country code, e.g. "ID", "CN", "DE".
pub fn valid_country(code: &str) -> bool {
    code.len() == 2 && code.bytes().all(|b| b.is_ascii_uppercase())
}

/// Harmonized System code: 6 to 10 digits, e.g. "731815" for bolts.
pub fn valid_hs_code(code: &str) -> bool {
    (6..=10).contains(&code.len()) && code.bytes().all(|b| b.is_ascii_digit())
}

/// The eleven Incoterms 2020 rules: who pays freight and carries the risk.
pub const INCOTERMS: [&str; 11] = [
    "EXW", "FCA", "FAS", "FOB", "CFR", "CIF", "CPT", "CIP", "DAP", "DPU", "DDP",
];

pub fn valid_incoterm(code: &str) -> bool {
    INCOTERMS.contains(&code)
}

/// "731815" -> "7318.15" as printed on customs papers.
pub fn hs_display(code: &str) -> String {
    if code.len() >= 6 {
        format!(
            "{}.{}{}",
            &code[..4],
            &code[4..6],
            if code.len() > 6 {
                format!(".{}", &code[6..])
            } else {
                String::new()
            }
        )
    } else {
        code.to_string()
    }
}

/// Escapes text for use inside SVG/XML.
pub fn xml_escape(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '&' => "&amp;".to_string(),
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '"' => "&quot;".to_string(),
            '\'' => "&#39;".to_string(),
            c => c.to_string(),
        })
        .collect()
}

/// Percent-encodes an SVG so it can sit in an NFT `media` field as a data URI.
pub fn svg_data_uri(svg: &str) -> String {
    let mut out = String::from("data:image/svg+xml;charset=utf-8,");
    for b in svg.bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~'
            | b' '
            | b'='
            | b':'
            | b'/'
            | b','
            | b';'
            | b'('
            | b')'
            | b'\'' => out.push(if b == b' ' { ' ' } else { b as char }),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out.replace(' ', "%20")
}

pub fn status_label(status: Status) -> (&'static str, &'static str) {
    match status {
        Status::Created => ("AWAITING PAYMENT", "#8FA0C8"),
        Status::Funded => ("MONEY LOCKED", "#E7B65C"),
        Status::ReadyForInspection => ("READY FOR INSPECTION", "#8FA0C8"),
        Status::Passed => ("PASSED", "#3DD9A5"),
        Status::Rejected => ("REJECTED", "#FF6B5B"),
        Status::Completed => ("COMPLETED", "#3DD9A5"),
    }
}

pub struct CardData<'a> {
    pub token_id: &'a str,
    pub product: &'a str,
    pub quantity: u64,
    pub hs_code: &'a str,
    pub incoterm: &'a str,
    pub origin: &'a str,
    pub destination: &'a str,
    pub supplier_name: &'a str,
    pub buyer_name: &'a str,
    pub inspector_name: &'a str,
    pub status: Status,
    pub held: u128,
    pub symbol: &'a str,
    pub decimals: u8,
    pub sample_size: u32,
    pub accept_max: u32,
    pub defects: Option<u32>,
}

fn clip(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        format!("{t}…")
    } else {
        t
    }
}

/// The passport card image stored in the NFT's `media`, so wallets show
/// each lot as a card. Same layout as the web app's gallery.
/// Product pictogram for the NFT card, chosen by the first four digits of the
/// HS code (96 x 96 viewBox, no outer <svg>). Unknown codes get a plain carton.
pub fn product_icon(hs_code: &str) -> &'static str {
    match hs_code.get(..4).unwrap_or("") {
        "8517" => r##"<g fill="none" stroke="#13233F" stroke-width="2" stroke-linejoin="round"><path d="M14 34l52-14 18 10-52 14z" fill="#CFDCE5"></path><path d="M14 34v22l18 10V44z" fill="#B9CAD6"></path><path d="M32 44l52-14v22L32 66z" fill="#DDE8EF"></path><path d="M40 39l40-11M44 42l38-10M48 45l35-9" opacity=".55"></path><rect x="17" y="44" width="5" height="7" rx="1" fill="#13233F"></rect><rect x="24" y="48" width="5" height="7" rx="1" fill="#13233F"></rect><path d="M14 56c-8 4-8 14 2 16l14-4" stroke-width="2.4"></path></g>"##,
        "8504" => r##"<g fill="none" stroke="#13233F" stroke-width="2" stroke-linejoin="round"><rect x="10" y="26" width="76" height="44" rx="4" fill="#CFDCE5"></rect><circle cx="34" cy="48" r="15" fill="#DDE8EF"></circle><circle cx="34" cy="48" r="3" fill="#13233F"></circle><path d="M34 33v30M19 48h30M23.4 37.4l21.2 21.2M44.6 37.4L23.4 58.6" opacity=".55"></path><rect x="58" y="38" width="18" height="12" rx="2" fill="#B9CAD6"></rect><path d="M62 42v4M67 42v4M72 42v4"></path><path d="M58 58h18" opacity=".6"></path></g>"##,
        "8419" => r##"<g fill="none" stroke="#13233F" stroke-width="2" stroke-linejoin="round"><rect x="16" y="24" width="64" height="48" rx="4" fill="#CFDCE5"></rect><path d="M26 36h44M26 44h44M26 52h44M26 60h44" opacity=".55"></path><circle cx="24" cy="18" r="5" fill="#B9CAD6"></circle><circle cx="72" cy="18" r="5" fill="#B9CAD6"></circle><path d="M24 23v4M72 23v4"></path></g>"##,
        "8473" => r##"<g fill="none" stroke="#13233F" stroke-width="2" stroke-linejoin="round"><rect x="10" y="30" width="76" height="34" rx="3" fill="#CFDCE5"></rect><rect x="18" y="36" width="12" height="16" rx="1" fill="#B9CAD6"></rect><rect x="34" y="36" width="12" height="16" rx="1" fill="#B9CAD6"></rect><rect x="50" y="36" width="12" height="16" rx="1" fill="#B9CAD6"></rect><rect x="66" y="36" width="12" height="16" rx="1" fill="#B9CAD6"></rect><path d="M14 64v6M22 64v6M30 64v6M38 64v6M46 64v6M54 64v6M62 64v6M70 64v6M78 64v6"></path></g>"##,
        "8507" => r##"<g fill="none" stroke="#13233F" stroke-width="2" stroke-linejoin="round"><rect x="14" y="30" width="62" height="36" rx="5" fill="#CFDCE5"></rect><rect x="76" y="41" width="7" height="14" rx="2" fill="#B9CAD6"></rect><rect x="20" y="36" width="11" height="24" rx="2" fill="#7FC9A8"></rect><rect x="34" y="36" width="11" height="24" rx="2" fill="#7FC9A8"></rect><rect x="48" y="36" width="11" height="24" rx="2" fill="#7FC9A8"></rect><rect x="62" y="36" width="9" height="24" rx="2" fill="#DDE8EF"></rect><path d="M44 18l-6 9h7l-5 9" stroke-width="2.4"></path></g>"##,
        "8544" => r##"<g fill="none" stroke="#13233F" stroke-width="2" stroke-linejoin="round"><ellipse cx="48" cy="50" rx="26" ry="18" stroke-width="5" stroke="#B9CAD6"></ellipse><ellipse cx="48" cy="50" rx="26" ry="18"></ellipse><ellipse cx="48" cy="50" rx="18" ry="11"></ellipse><rect x="10" y="20" width="18" height="12" rx="2" fill="#CFDCE5"></rect><rect x="68" y="66" width="18" height="12" rx="2" fill="#CFDCE5"></rect><path d="M28 26c8 0 10 8 12 12M68 72c-8 0-10-8-12-12"></path></g>"##,
        "8537" => r##"<g fill="none" stroke="#13233F" stroke-width="2" stroke-linejoin="round"><rect x="34" y="8" width="28" height="80" rx="4" fill="#CFDCE5"></rect><rect x="40" y="14" width="16" height="8" rx="1" fill="#7FC9A8"></rect><g fill="#DDE8EF"><rect x="40" y="28" width="16" height="10" rx="2"></rect><rect x="40" y="42" width="16" height="10" rx="2"></rect><rect x="40" y="56" width="16" height="10" rx="2"></rect><rect x="40" y="70" width="16" height="10" rx="2"></rect></g><path d="M45 31v4M51 31v4M45 45v4M51 45v4M45 59v4M51 59v4M45 73v4M51 73v4"></path></g>"##,
        "8414" => r##"<g fill="none" stroke="#13233F" stroke-width="2" stroke-linejoin="round"><rect x="14" y="14" width="68" height="68" rx="8" fill="#CFDCE5"></rect><circle cx="48" cy="48" r="28" fill="#DDE8EF"></circle><circle cx="48" cy="48" r="7" fill="#B9CAD6"></circle><path d="M48 41c-2-10 4-16 12-14-2 8-6 12-12 14zM55 48c10-2 16 4 14 12-8-2-12-6-14-12zM48 55c2 10-4 16-12 14 2-8 6-12 12-14zM41 48c-10 2-16-4-14-12 8 2 12 6 14 12z" fill="#B9CAD6"></path><circle cx="21" cy="21" r="2.5"></circle><circle cx="75" cy="21" r="2.5"></circle><circle cx="21" cy="75" r="2.5"></circle><circle cx="75" cy="75" r="2.5"></circle></g>"##,
        "8471" => r##"<g fill="none" stroke="#13233F" stroke-width="2" stroke-linejoin="round"><rect x="20" y="12" width="56" height="72" rx="5" fill="#CFDCE5"></rect><rect x="28" y="22" width="40" height="24" rx="2" fill="#DDE8EF"></rect><path d="M34 30h28M34 36h20" opacity=".6"></path><rect x="28" y="54" width="18" height="18" rx="2" fill="#B9CAD6"></rect><rect x="50" y="54" width="18" height="8" rx="1" fill="#B9CAD6"></rect><path d="M30 84v4M36 84v4M42 84v4M48 84v4M54 84v4M60 84v4M66 84v4"></path></g>"##,
        "8413" => r##"<g fill="none" stroke="#13233F" stroke-width="2" stroke-linejoin="round"><rect x="22" y="10" width="52" height="76" rx="4" fill="#CFDCE5"></rect><rect x="30" y="18" width="36" height="14" rx="2" fill="#DDE8EF"></rect><path d="M34 25h12" stroke="#7FC9A8" stroke-width="3"></path><circle cx="48" cy="56" r="14" fill="#DDE8EF"></circle><path d="M48 46v20M38 56h20" opacity=".6"></path><path d="M74 42h10v6H74M74 66h10v6H74" fill="#B9CAD6"></path><path d="M12 42h10M12 72h10" stroke-width="3" stroke="#8FB9DE"></path></g>"##,
        _ => r##"<g fill="none" stroke="#13233F" stroke-width="2" stroke-linejoin="round"><path d="M14 34l52-14 18 10-52 14z" fill="#CFDCE5"/><path d="M14 34v22l18 10V44z" fill="#B9CAD6"/><path d="M32 44l52-14v22L32 66z" fill="#DDE8EF"/></g>"##,
    }
}

pub fn passport_svg(d: &CardData) -> String {
    let (label, color) = status_label(d.status);
    let product = xml_escape(&clip(d.product, 18).to_uppercase());
    let result = match d.defects {
        Some(x) => format!("{x} / {} DEFECTIVE · LIMIT {}", d.sample_size, d.accept_max),
        None => format!("PLAN {} SAMPLES · LIMIT {}", d.sample_size, d.accept_max),
    };
    let mut rings = String::new();
    for i in 0..9 {
        rings.push_str(&format!(r##"<ellipse cx="470" cy="150" rx="{}" ry="{}" fill="none" stroke="#E7B65C" stroke-opacity="0.09"/>"##, 60 + i * 16, 40 + i * 11));
    }
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="600" height="380" viewBox="0 0 600 380"><rect width="600" height="380" rx="28" fill="#0F1B3D"/><rect x="14" y="14" width="572" height="352" rx="18" fill="none" stroke="#E7B65C" stroke-opacity="0.35"/>{rings}<text x="40" y="56" font-family="monospace" font-size="13" letter-spacing="4" fill="#E7B65C">LOT PASSPORT · NEP-171</text><rect x="432" y="80" width="104" height="104" rx="18" fill="#DDE8EF"/><g transform="translate(441 89) scale(0.9)">{icon}</g><text x="560" y="56" text-anchor="end" font-family="monospace" font-size="22" fill="#EEF1FA">#{id}</text><text x="40" y="104" font-family="sans-serif" font-weight="800" font-size="28" fill="#EEF1FA">{product}</text><text x="40" y="132" font-family="monospace" font-size="14" fill="#A7B1CC">HS {hs} · {qty} PCS · {inc}</text><text x="40" y="168" font-family="monospace" font-size="13" fill="#EEF1FA">FROM {sup} ({o})</text><text x="40" y="188" font-family="monospace" font-size="13" fill="#EEF1FA">TO   {buy} ({de})</text><text x="40" y="226" font-family="monospace" font-size="12" letter-spacing="2" fill="#A7B1CC">HOLDS</text><text x="40" y="260" font-family="sans-serif" font-weight="800" font-size="32" fill="#E7B65C">{held} {sym}</text><text x="40" y="304" font-family="monospace" font-size="12.5" fill="#A7B1CC">ISO 2859-1 AQL 2.5 · {result}</text><text x="40" y="328" font-family="monospace" font-size="12.5" fill="#A7B1CC">INSPECTED BY {insp}</text><g transform="rotate(-10 470 262)"><rect x="380" y="232" width="180" height="58" rx="10" fill="none" stroke="{color}" stroke-width="4"/><text x="470" y="270" text-anchor="middle" font-family="sans-serif" font-weight="800" font-size="{fs}" fill="{color}">{label}</text></g></svg>"##,
        rings = rings,
        icon = product_icon(d.hs_code),
        id = xml_escape(d.token_id),
        product = product,
        hs = xml_escape(&hs_display(d.hs_code)),
        qty = d.quantity,
        inc = xml_escape(d.incoterm),
        sup = xml_escape(&clip(d.supplier_name, 34).to_uppercase()),
        o = xml_escape(d.origin),
        buy = xml_escape(&clip(d.buyer_name, 34).to_uppercase()),
        de = xml_escape(d.destination),
        held = format_amount(d.held, d.decimals),
        sym = xml_escape(d.symbol),
        result = result,
        insp = xml_escape(&clip(d.inspector_name, 34).to_uppercase()),
        color = color,
        label = label,
        fs = if label.len() > 14 { 13 } else if label.len() > 9 { 19 } else { 24 },
    )
}

/// HS code prefix a supplier declares or searches by: 4 to 10 digits ("8517", "851762").
pub fn valid_hs_prefix(code: &str) -> bool {
    (4..=10).contains(&code.len()) && code.bytes().all(|b| b.is_ascii_digit())
}

/// The facts about one lot that count toward a supplier's track record.
pub struct LotFacts {
    pub status: Status,
    pub quantity: u64,
    pub sample_size: u32,
    pub defects: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackRecord {
    pub lots: u32,
    pub passed: u32,
    pub rejected: u32,
    pub in_progress: u32,
    /// Units in lots that passed inspection.
    pub units_passed: u64,
    /// Defective pieces per million sampled, over all inspected lots.
    pub defect_ppm: Option<u32>,
    /// Pass rate with one imaginary pass and one imaginary fail added
    /// (Laplace smoothing), in basis points: a new supplier starts at 5,000
    /// and one lucky lot cannot jump it to the top.
    pub score_bp: u32,
}

/// Builds a supplier's track record from its passports. Only inspected lots
/// count toward the score, so nobody can rank by minting unpaid lots.
pub fn track_record(lots: &[LotFacts]) -> TrackRecord {
    let (mut passed, mut rejected, mut in_progress) = (0u32, 0u32, 0u32);
    let (mut units_passed, mut sampled, mut defects) = (0u64, 0u64, 0u64);
    for l in lots {
        match l.status {
            Status::Passed | Status::Completed => {
                passed += 1;
                units_passed += l.quantity;
            }
            Status::Rejected => rejected += 1,
            _ => in_progress += 1,
        }
        if let Some(d) = l.defects {
            sampled += l.sample_size as u64;
            defects += d as u64;
        }
    }
    let inspected = (passed + rejected) as u64;
    TrackRecord {
        lots: lots.len() as u32,
        passed,
        rejected,
        in_progress,
        units_passed,
        defect_ppm: if sampled > 0 {
            Some((defects * 1_000_000 / sampled) as u32)
        } else {
            None
        },
        score_bp: (((passed as u64) + 1) * 10_000 / (inspected + 2)) as u32,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn card_picture_follows_hs_code() {
        let optic = product_icon("851762");
        let fan = product_icon("841459");
        let pump = product_icon("841370");
        assert_ne!(optic, fan);
        assert_ne!(fan, pump);
        assert_eq!(product_icon("999999"), product_icon("12"));
        assert!(!optic.contains("<svg"));
    }

    use super::*;

    #[test]
    fn plan_for_demo_lot() {
        // 6,900 pieces -> code L, 200 samples, accept up to 10.
        assert_eq!(
            aql_plan(6_900),
            SamplingPlan {
                sample_size: 200,
                accept_max: 10
            }
        );
    }

    #[test]
    fn plan_table_points() {
        assert_eq!(
            aql_plan(100),
            SamplingPlan {
                sample_size: 20,
                accept_max: 1
            }
        );
        assert_eq!(
            aql_plan(400),
            SamplingPlan {
                sample_size: 50,
                accept_max: 3
            }
        );
        assert_eq!(
            aql_plan(1_000),
            SamplingPlan {
                sample_size: 80,
                accept_max: 5
            }
        );
        assert_eq!(
            aql_plan(20_000),
            SamplingPlan {
                sample_size: 315,
                accept_max: 14
            }
        );
        assert_eq!(
            aql_plan(200_000),
            SamplingPlan {
                sample_size: 500,
                accept_max: 21
            }
        );
    }

    #[test]
    fn tiny_lot_is_fully_inspected() {
        assert_eq!(
            aql_plan(3),
            SamplingPlan {
                sample_size: 3,
                accept_max: 0
            }
        );
        assert_eq!(
            aql_plan(40),
            SamplingPlan {
                sample_size: 5,
                accept_max: 0
            }
        );
    }

    #[test]
    fn cartons_round_up() {
        assert_eq!(carton_count(6_900, 50), 138);
        assert_eq!(carton_count(6_901, 50), 139);
    }

    #[test]
    fn draw_is_deterministic_distinct_and_in_range() {
        let seed = [7u8; 32];
        let a = pick_cartons(&seed, 138, CARTONS_TO_OPEN);
        let b = pick_cartons(&seed, 138, CARTONS_TO_OPEN);
        assert_eq!(a, b);
        assert_eq!(a.len(), 6);
        assert!(a.iter().all(|c| (1..=138).contains(c)));
        let mut d = a.clone();
        d.dedup();
        assert_eq!(d.len(), a.len());
        assert_ne!(a, pick_cartons(&[8u8; 32], 138, CARTONS_TO_OPEN));
    }

    #[test]
    fn draw_never_asks_for_more_cartons_than_exist() {
        assert_eq!(pick_cartons(&[1u8; 32], 3, CARTONS_TO_OPEN), vec![1, 2, 3]);
    }

    #[test]
    fn verdict_follows_plan() {
        let p = aql_plan(6_900);
        assert_eq!(verdict(p, 200, 4), Ok(true));
        assert_eq!(verdict(p, 200, 10), Ok(true));
        assert_eq!(verdict(p, 200, 11), Ok(false));
        assert!(verdict(p, 150, 0).is_err());
        assert!(verdict(p, 200, 201).is_err());
    }

    #[test]
    fn split_keeps_ten_percent() {
        // $4,278 in USDC units (6 decimals).
        assert_eq!(split_on_pass(4_278_000_000), (3_850_200_000, 427_800_000));
        let (now, kept) = split_on_pass(999);
        assert_eq!(now + kept, 999);
    }

    #[test]
    fn release_rules() {
        assert!(can_release(0, 100, true));
        assert!(!can_release(99, 100, false));
        assert!(can_release(100, 100, false));
    }

    #[test]
    fn transfer_only_after_settlement_with_nothing_inside() {
        assert!(!can_transfer(Status::Funded, 4_278, 0));
        // A sale during the claim window would carry the 10% along: refused.
        assert!(!can_transfer(Status::Passed, 427, 0));
        assert!(can_transfer(Status::Completed, 0, 0));
        assert!(can_transfer(Status::Rejected, 0, 0));
        // A payout that failed and waits for retry also blocks the transfer.
        assert!(!can_transfer(Status::Completed, 0, 1));
        // Defensive: settled status but money still recorded inside.
        assert!(!can_transfer(Status::Completed, 1, 0));
    }

    #[test]
    fn pagination() {
        assert_eq!(page_range(10, None, None), (0, 10));
        assert_eq!(page_range(10, Some(4), Some(3)), (4, 7));
        assert_eq!(page_range(10, Some(20), Some(3)), (10, 10));
        assert_eq!(page_range(500, None, Some(1_000)), (0, 100));
    }

    #[test]
    fn amount_formatting() {
        assert_eq!(format_amount(4_278_000_000, 6), "4,278.00");
        assert_eq!(format_amount(427_800_000, 6), "427.80");
        assert_eq!(format_amount(0, 6), "0.00");
        assert_eq!(format_amount(1_234_567_890_000, 6), "1,234,567.89");
        // wNEAR has 24 decimals
        assert_eq!(
            format_amount(15_500_000_000_000_000_000_000_000, 24),
            "15.50"
        );
        assert_eq!(format_amount(7, 0), "7.00");
    }

    #[test]
    fn trade_field_validation() {
        assert!(valid_country("ID") && valid_country("CN"));
        assert!(!valid_country("IDN") && !valid_country("id"));
        assert!(valid_hs_code("731815") && valid_hs_code("7318150000"));
        assert!(!valid_hs_code("7318.15") && !valid_hs_code("7318"));
        assert!(valid_incoterm("FOB") && valid_incoterm("DDP"));
        assert!(!valid_incoterm("FOBX") && !valid_incoterm("fob"));
        assert_eq!(hs_display("731815"), "7318.15");
        assert_eq!(hs_display("7318150010"), "7318.15.0010");
    }

    #[test]
    fn svg_is_escaped_and_encoded() {
        let d = CardData {
            token_id: "0001",
            product: "Bolts <M8> & nuts",
            quantity: 6_900,
            hs_code: "731815",
            incoterm: "FOB",
            origin: "CN",
            destination: "ID",
            supplier_name: "Ningbo Haitong Fasteners Co., Ltd.",
            buyer_name: "PT Maju Teknik",
            inspector_name: "V-Trust Inspection",
            status: Status::Passed,
            held: 427_800_000,
            symbol: "USDC",
            decimals: 6,
            sample_size: 200,
            accept_max: 10,
            defects: Some(4),
        };
        let svg = passport_svg(&d);
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("BOLTS &lt;M8&gt; &amp; NUTS"));
        assert!(svg.contains("427.80 USDC"));
        assert!(svg.contains("HS 7318.15"));
        assert!(svg.contains("(CN)") && svg.contains("(ID)"));
        assert!(svg.contains(">PASSED<"));
        let uri = svg_data_uri(&svg);
        assert!(uri.starts_with("data:image/svg+xml;charset=utf-8,%3Csvg"));
        assert!(
            !uri.contains('<') && !uri.contains('#') && !uri.contains(' ') && !uri.contains('"')
        );
    }

    #[test]
    fn track_record_counts_only_inspected_lots() {
        let lot = |status, defects| LotFacts {
            status,
            quantity: 2_400,
            sample_size: 125,
            defects,
        };
        let new_supplier = track_record(&[]);
        assert_eq!(new_supplier.score_bp, 5_000);
        assert_eq!(new_supplier.defect_ppm, None);

        let r = track_record(&[
            lot(Status::Completed, Some(2)),
            lot(Status::Passed, Some(3)),
            lot(Status::Rejected, Some(12)),
            lot(Status::Funded, None),
        ]);
        assert_eq!((r.lots, r.passed, r.rejected, r.in_progress), (4, 2, 1, 1));
        assert_eq!(r.units_passed, 4_800);
        assert_eq!(r.defect_ppm, Some(17 * 1_000_000 / 375));
        assert_eq!(r.score_bp, 3 * 10_000 / 5);

        // One lucky lot does not beat a long clean record.
        let lucky = track_record(&[lot(Status::Completed, Some(0))]);
        let steady: Vec<LotFacts> = (0..10).map(|_| lot(Status::Completed, Some(1))).collect();
        assert!(track_record(&steady).score_bp > lucky.score_bp);
    }

    #[test]
    fn hs_prefix_rule() {
        assert!(valid_hs_prefix("8517") && valid_hs_prefix("851762"));
        assert!(!valid_hs_prefix("851") && !valid_hs_prefix("85.17"));
    }
}

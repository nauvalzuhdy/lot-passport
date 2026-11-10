#!/usr/bin/env bash
# Lot Passport: the full happy path on NEAR testnet with near-cli-rs.
# 1) Create the accounts below (README step 1). 2) Edit the names. 3) Run from the repo root.
# Check each command against `near --help` if your near-cli-rs version differs.
set -euo pipefail

CONTRACT=nauval-lp-contract.testnet
BUYER=nauval-lp-nordlicht-rz.testnet
SUPPLIER=nauval-lp-lightwave-optics.testnet
INSPECTOR=nauval-lp-v-trust.testnet
CUSTOMER=nauval-lp-kestrel-cloud.testnet
TOKEN_KIND=${TOKEN_KIND:-usdc}          # usdc (Circle test USDC) or wnear (wrapped testnet NEAR)
if [ "$TOKEN_KIND" = "usdc" ]; then
  TOKEN=3e2210e1184b45b64c8a434c0a7e7b23cc04ea7eb7a6c3c32520d03d4afcb8af; SYMBOL=USDC; DECIMALS=6
  PRICE=15000000                        # 15 USDC (6 decimals)
else
  TOKEN=wrap.testnet; SYMBOL=wNEAR; DECIMALS=24
  PRICE=15000000000000000000000000      # 15 wNEAR (24 decimals)
fi

call () { # call <signer> <receiver> <method> <json-args> <deposit> [gas]
  near contract call-function as-transaction "$2" "$3" json-args "$4" \
    prepaid-gas "${6:-100.0 Tgas}" attached-deposit "$5" sign-as "$1" \
    network-config testnet sign-with-keychain send
}
view () { near contract call-function as-read-only "$CONTRACT" "$1" json-args "$2" network-config testnet now; }

echo "== Build and deploy"
(cd contract && cargo near build non-reproducible-wasm --no-abi)
near contract deploy "$CONTRACT" use-file contract/target/near/lot_passport.wasm \
  with-init-call new json-args "{\"token\":\"$TOKEN\",\"token_symbol\":\"$SYMBOL\",\"token_decimals\":$DECIMALS}" \
  prepaid-gas '100.0 Tgas' attached-deposit '0 NEAR' network-config testnet sign-with-keychain send

echo "== Register contract, buyer and supplier with the token"
for acc in "$CONTRACT" "$BUYER" "$SUPPLIER"; do
  call "$BUYER" "$TOKEN" storage_deposit "{\"account_id\":\"$acc\",\"registration_only\":true}" '0.0125 NEAR' '30.0 Tgas'
done
if [ "$TOKEN_KIND" = "usdc" ]; then
  echo "== Get 15+ test USDC for $BUYER at https://faucet.circle.com (network: NEAR Testnet), then press Enter"
  read -r _
else
  call "$BUYER" "$TOKEN" near_deposit '{}' '20 NEAR' '30.0 Tgas'
fi

echo "== Each company registers once"
call "$BUYER"     "$CONTRACT" register_company '{"legal_name":"Nordlicht Rechenzentren GmbH","country":"DE","city":"Frankfurt am Main","registration_hash":null,"hs_codes":[]}' '0.02 NEAR' '30.0 Tgas'
call "$SUPPLIER"  "$CONTRACT" register_company '{"legal_name":"Shenzhen Lightwave Optics Co., Ltd.","country":"CN","city":"Shenzhen","registration_hash":null,"hs_codes":["8517"]}' '0.02 NEAR' '30.0 Tgas'
call "$INSPECTOR" "$CONTRACT" register_company '{"legal_name":"V-Trust Inspection","country":"CN","city":"Shenzhen","registration_hash":null,"hs_codes":[]}' '0.02 NEAR' '30.0 Tgas'

echo "== 1. create_lot (buyer)"
call "$BUYER" "$CONTRACT" create_lot "{\"supplier\":\"$SUPPLIER\",\"inspector\":\"$INSPECTOR\",\"goods\":{\"description\":\"800G OSFP optical transceivers\",\"hs_code\":\"851762\",\"quantity\":2400,\"units_per_carton\":20,\"origin_country\":\"CN\",\"destination_country\":\"DE\",\"incoterm\":\"FOB\"},\"price\":\"$PRICE\"}" '0.05 NEAR' '50.0 Tgas'
LOT=0001

echo "== Pay into the passport (token calls ft_on_transfer, msg = lot id)"
call "$BUYER" "$TOKEN" ft_transfer_call "{\"receiver_id\":\"$CONTRACT\",\"amount\":\"$PRICE\",\"msg\":\"$LOT\"}" '1 yoctoNEAR'

echo "== Supplier finishes production"
call "$SUPPLIER" "$CONTRACT" mark_ready "{\"token_id\":\"$LOT\"}" '0 NEAR'

echo "== 2. draw_sample (inspector, at the factory)"
call "$INSPECTOR" "$CONTRACT" draw_sample "{\"token_id\":\"$LOT\"}" '0 NEAR'

echo "== Security check: the buyer trying to stamp the passport must FAIL"
if call "$BUYER" "$CONTRACT" submit_inspection "{\"token_id\":\"$LOT\",\"sampled\":125,\"defects\":0,\"report_hash\":\"fake\"}" '0 NEAR'; then
  echo "UNEXPECTED: buyer could stamp"; exit 1
else
  echo "OK: refused, only the inspector can stamp"
fi

echo "== 3. submit_inspection: 2 failed modules in 125 -> pass, 90% paid now"
call "$INSPECTOR" "$CONTRACT" submit_inspection "{\"token_id\":\"$LOT\",\"sampled\":125,\"defects\":2,\"report_hash\":\"sha256:replace-with-your-report-hash\"}" '0 NEAR'

echo "== Security check: selling the passport while 10% is still inside must FAIL"
if call "$BUYER" "$CONTRACT" nft_transfer "{\"receiver_id\":\"$CUSTOMER\",\"token_id\":\"$LOT\"}" '1 yoctoNEAR' '30.0 Tgas'; then
  echo "UNEXPECTED: passport sold during the claim window"; exit 1
else
  echo "OK: refused, a passport changes hands only when nothing is left inside"
fi

echo "== 4. release (buyer confirms early; anyone can after 7 days)"
call "$BUYER" "$CONTRACT" release "{\"token_id\":\"$LOT\"}" '0 NEAR'

echo "== 5. transfer the passport to the buyer's customer (NEP-171)"
call "$BUYER" "$CONTRACT" nft_transfer "{\"receiver_id\":\"$CUSTOMER\",\"token_id\":\"$LOT\"}" '1 yoctoNEAR' '30.0 Tgas'

echo "== Result"
view get_lot "{\"token_id\":\"$LOT\"}"
view lots_for_supplier "{\"account_id\":\"$SUPPLIER\"}"
echo "== Find suppliers of 8517 (telecom/optics) in China, ranked by passport record"
view find_suppliers '{"hs_prefix":"8517","country":"CN","limit":10}'

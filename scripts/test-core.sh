#!/usr/bin/env bash
# Runs the contract's pure logic tests (AQL plan, carton draw, verdict, payout split,
# trade-field validation, passport image) with plain rustc: no NEAR toolchain needed.
set -euo pipefail
cd "$(dirname "$0")/.."
rustc --edition 2021 --cfg standalone --test contract/src/core.rs -o target-core-tests
./target-core-tests
rm -f target-core-tests

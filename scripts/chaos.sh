#!/usr/bin/env bash
# Chaos test (T065; D-003, term:fail-safe-degradation).
#
# Faults the online feature store under load and asserts that fail-safe degradation holds and the
# 20 ms p99 SLA is still met. The load harness (benches/load_sla.rs) returns non-zero if p99
# breaches the SLA or if degradation failed to engage, so `set -e` turns either into a failure here.
set -euo pipefail

cd "$(dirname "$0")/.."

echo "== chaos: faulting the online feature store under load =="
LOAD_SLA_FAULT=store cargo bench -p engine --bench load_sla

echo "== chaos: passed — fail-safe degraded to rules-only and p99 stayed within the 20ms SLA =="

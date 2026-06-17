#!/usr/bin/env bash
# Integration tests against live dockerised Postgres + Redis (the persistence boundaries).
#
# These exercise PostgresAuditSink (append-only audit log, incl. the DB immutability trigger) and
# RedisFeatureStore (online feature store round-trip) — the `#[ignore]`d tests that need real
# services. Run: scripts/integration-test.sh
#
# VPN note (Windscribe): the kill-switch firewall DROPs host<->docker-bridge traffic, so a published
# port accepts the TCP SYN (via docker-proxy) but the session then hangs (~133s). We insert an
# idempotent ACCEPT for the docker bridge subnet (172.16.0.0/12) ahead of the windscribe_block chain.
# These rules are ephemeral — Windscribe may re-apply its chains on reconnect; just re-run this script.
set -euo pipefail
cd "$(dirname "$0")/.."

PG_PORT="${POSTGRES_PORT:-55432}"
RD_PORT="${REDIS_PORT:-56379}"
DOCKER_SUBNET="172.16.0.0/12"

echo "== bringing up postgres + redis (ports $PG_PORT / $RD_PORT) =="
POSTGRES_PORT="$PG_PORT" REDIS_PORT="$RD_PORT" docker compose up -d postgres redis >/dev/null

# Allow host<->docker-bridge through any VPN kill-switch (Windscribe). Idempotent; needs sudo when
# run manually. Skips cleanly if iptables/sudo are unavailable.
if command -v iptables >/dev/null 2>&1; then
  echo "== ensuring docker bridge subnet $DOCKER_SUBNET is allowed past the VPN firewall (sudo) =="
  sudo iptables -C INPUT  -s "$DOCKER_SUBNET" -j ACCEPT 2>/dev/null || sudo iptables -I INPUT  1 -s "$DOCKER_SUBNET" -j ACCEPT || true
  sudo iptables -C OUTPUT -d "$DOCKER_SUBNET" -j ACCEPT 2>/dev/null || sudo iptables -I OUTPUT 1 -d "$DOCKER_SUBNET" -j ACCEPT || true
fi

echo "== waiting for postgres health =="
for _ in $(seq 1 30); do
  [ "$(docker inspect --format '{{.State.Health.Status}}' node-sec-postgres-1 2>/dev/null)" = "healthy" ] && break
  sleep 2
done

export NODESEC_PG="host=127.0.0.1 port=$PG_PORT user=nodesec password=nodesec dbname=nodesec"
export NODESEC_REDIS="redis://127.0.0.1:$RD_PORT"

echo "== running #[ignore]d integration tests =="
cargo test -p compliance --lib postgres_sink_persists_a_record -- --ignored
cargo test -p stream --lib redis_store_round_trips -- --ignored

echo "== integration tests passed =="

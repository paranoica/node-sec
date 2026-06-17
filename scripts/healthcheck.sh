#!/usr/bin/env bash
# Wait for all docker-compose services to report healthy (T003 verify handle).
# Usage: scripts/healthcheck.sh [timeout_seconds]
set -euo pipefail

timeout="${1:-120}"
services="redpanda redis postgres prometheus grafana"
deadline=$(( $(date +%s) + timeout ))

echo "waiting up to ${timeout}s for: ${services}"
while :; do
  all_healthy=1
  for svc in $services; do
    cid="$(docker compose ps -q "$svc" 2>/dev/null || true)"
    if [ -z "$cid" ]; then
      all_healthy=0
      break
    fi
    status="$(docker inspect -f '{{ if .State.Health }}{{ .State.Health.Status }}{{ else }}{{ .State.Status }}{{ end }}' "$cid" 2>/dev/null || echo unknown)"
    if [ "$status" != "healthy" ] && [ "$status" != "running" ]; then
      all_healthy=0
      break
    fi
  done
  if [ "$all_healthy" -eq 1 ]; then
    echo "all services healthy"
    docker compose ps
    exit 0
  fi
  if [ "$(date +%s)" -ge "$deadline" ]; then
    echo "ERROR: services did not become healthy within ${timeout}s" >&2
    docker compose ps >&2
    exit 1
  fi
  sleep 3
done

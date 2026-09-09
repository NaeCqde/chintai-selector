#!/usr/bin/env bash
# Fills in the real Google-Maps-walked time to each property's listed station.
# Two requests per property (the /maps/dir/ page + its preview/directions
# link), paced by HttpClient's per-host limiter — this is the *only* route
# this project pre-computes in bulk; routes to a user's own places are always
# computed live by the engine at search time.
#
# Run once first (local only, no network):
#   ingest backfill-station-names --limit 200000
# Then this, which just keeps calling `ingest google-stations` until the
# pending queue is empty (each call is one bounded, safely-resumable pass).
#
#   CHINTAI_DB="sqlite://chintai.sqlite?mode=rwc" scripts/crawl_google_stations.sh
set -uo pipefail
cd "$(dirname "$0")/.."

BIN=./target/debug/ingest
export RUST_LOG="${RUST_LOG:-info}"
BATCH="${BATCH:-500}"

while true; do
    out=$("$BIN" google-stations --limit "$BATCH" 2>&1)
    echo "$out"
    pending=$(echo "$out" | grep -oE '[0-9]+ properties pending' | grep -oE '^[0-9]+')
    if [ "${pending:-0}" -eq 0 ]; then
        echo "google-stations: queue empty, done"
        break
    fi
done

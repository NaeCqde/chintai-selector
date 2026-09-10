#!/usr/bin/env bash
# Fills in the real Google-Maps walk AND car time from each property to its
# listed station (car leg is for 送迎モード — someone drives you to the
# station, then transit from there). Paced by HttpClient's per-host limiter
# (see chintai_scraper::pacing) — this is the *only* route this project
# pre-computes in bulk; routes to a user's own places are always computed
# live by the engine at search time.
#
# Run once first (local only, no network):
#   ingest backfill-station-names --limit 200000
# Then this, which just keeps calling `ingest google-stations` until both the
# main queue and the car-time catch-up queue (properties whose walk leg
# resolved before car-time support existed) are empty. Each call is one
# bounded, safely-resumable pass.
#
#   CHINTAI_DB="sqlite://chintai.sqlite?mode=rwc" scripts/crawl_google_stations.sh
set -uo pipefail
cd "$(dirname "$0")/.."

BIN="${BIN:-./target/debug/ingest}"
export RUST_LOG="${RUST_LOG:-info}"
BATCH="${BATCH:-500}"

while true; do
    out=$("$BIN" google-stations --limit "$BATCH" 2>&1)
    echo "$out"
    pending=$(echo "$out" | grep -oE '[0-9]+ (station rows|properties) pending' | grep -oE '^[0-9]+')
    backfill=$(echo "$out" | grep -oE '[0-9]+ station rows need car-time backfill' | grep -oE '^[0-9]+')
    if [ "${pending:-0}" -eq 0 ] && [ "${backfill:-0}" -eq 0 ]; then
        echo "google-stations: queue empty, done"
        break
    fi
done

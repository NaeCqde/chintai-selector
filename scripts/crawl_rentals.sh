#!/usr/bin/env bash
# Complete rental crawl for Tokyo + Kanagawa. Per-site pace is tuned in
# `chintai_scraper::pacing` (scraper/src/pacing.rs), not on this command line.
# Personal use. Run from the repo root after `cargo build -p chintai-scraper`.
#
#   CHINTAI_DB="sqlite://chintai.sqlite?mode=rwc" scripts/crawl_rentals.sh
set -uo pipefail
cd "$(dirname "$0")/.."

BIN=./target/debug/ingest
export RUST_LOG="${RUST_LOG:-info}"
PAGES="${PAGES:-250}"

# --- SUUMO: every ward / city sub-area of Tokyo and Kanagawa ---------------
SUUMO_TOKYO="adachi akiruno akishima arakawa bunkyo chiyoda chofu chuo edogawa \
fuchu fussa hachioji hamura higashikurume higashimurayama higashiyamato hino \
inagi itabashi katsushika kita kiyose kodaira koganei kokubunji komae koto \
kunitachi machida meguro minato mitaka musashimurayama musashino nakano nerima \
nishitamagun nishitokyo ome ota setagaya shibuya shinagawa shinjuku suginami \
sumida tachikawa taito tama toshima"

SUUMO_KANAGAWA="aikogun ashigarakamigun ashigarashimogun atsugi ayase chigasaki \
ebina fujisawa hadano hiratsuka isehara kamakura kawasakishiasao \
kawasakishikawasaki kawasakishimiyamae kawasakishinakahara kawasakishisaiwai \
kawasakishitakatsu kawasakishitama kozagun minamiashigara miura miuragun \
nakagun odawara sagamiharashichuo sagamiharashimidori sagamiharashiminami \
yamato yokohamashiaoba yokohamashiasahi yokohamashihodogaya yokohamashiisogo \
yokohamashiizumi yokohamashikanagawa yokohamashikanazawa yokohamashikohoku \
yokohamashikonan yokohamashimidori yokohamashiminami yokohamashinaka \
yokohamashinishi yokohamashisakae yokohamashiseya yokohamashitotsuka \
yokohamashitsurumi yokohamashitsuzuki yokosuka zama zushi"

# --- HOME'S / AtHome: <romaji>-city (both use this convention) ------------
# HOME'S only reliably serves the 23 special wards this way (Tama/Kanagawa
# under this pattern 403/202'd during testing); AtHome's URL routing accepted
# every city we tried (Tokyo wards, Tama, Yokohama, Kawasaki), so it gets the
# full list. AtHome also has bot-defense that a burst of rapid manual testing
# tripped during development (a puzzle-challenge page instead of listings) —
# its pace lives at `pacing::ATHOME`, separate from every other site, so it
# can be slowed down on its own without touching SUUMO/HOME'S/Google Maps/etc.
# if a sustained crawl starts coming back with 0 buildings across the board.
HOMES_TOKYO_WARDS="chiyoda chuo minato shinjuku bunkyo taito sumida koto \
shinagawa meguro ota setagaya shibuya nakano suginami toshima kita arakawa \
itabashi nerima adachi katsushika edogawa"

for w in $SUUMO_TOKYO;    do "$BIN" rentals --pages "$PAGES" --suumo "tokyo/sc_$w";    done
for w in $SUUMO_KANAGAWA; do "$BIN" rentals --pages "$PAGES" --suumo "kanagawa/sc_$w"; done
for w in $HOMES_TOKYO_WARDS; do "$BIN" rentals --pages "$PAGES" --homes "tokyo/$w-city"; done
for w in $SUUMO_TOKYO;    do "$BIN" rentals --pages "$PAGES" --athome "tokyo/$w-city";    done
for w in $SUUMO_KANAGAWA; do "$BIN" rentals --pages "$PAGES" --athome "kanagawa/$w-city"; done

# Geocode whatever the station-name heuristic didn't resolve.
"$BIN" geocode --limit 20000

echo "crawl complete"

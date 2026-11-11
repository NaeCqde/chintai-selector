# chintai-selector

[日本語](README.md) | English

> [!WARNING]
> **This project is work in progress. It is extremely heavy (very slow) and not practical yet.**
>
> **Scraping property sites (SUUMO, HOME'S, and so on) is at your own risk.** Check the terms of use and the robots.txt of each site,
> keep the rate low and limit it to personal use. The author takes no responsibility for problems caused by scraping.

A rental search helper for choosing a home from the places you go to (like Trivago in the hotel business).

Register your workplace, school and other places you often visit, and for each of them set the means of travel (on foot only / car / public transport plus walking)
and the longest travel time. From the properties collected from existing rental sites, those that pass every condition are ranked by
speed of access, rent, floor area and so on, and shown on a map together with the destinations. For personal use.

## Structure

A Rust workspace.

| Crate | Role | Depends on |
|---|---|---|
| `types` (`chintai-types`) | Data types with serde only (wasm-capable), shared by the API and the WASM | none |
| `db` (`chintai-db`) | SQLite persistence layer (SeaORM): schema and repositories | none |
| `scraper` (`chintai-scraper`) | Data collection: route search, POI (Overpass), rentals (SUUMO/HOME'S). Only `reqwest`. The `ingest` binary | `db`, `types` |
| `server` (`chintai-server`) | JSON API, filter/ranking engine, static delivery of the WASM | `db`, `scraper`, `types` |
| `web` (`chintai-web`) | Leptos CSR SPA (wasm32). MapLibre GL + OSM tiles | `types` |

`db` and `scraper` do not depend on `server`.

## Development

```sh
cargo build --workspace --exclude chintai-web
cargo test  --workspace --exclude chintai-web

# Loading data (personal use, low rate)
export CHINTAI_DB="sqlite://chintai.sqlite?mode=rwc"
cargo run -p chintai-scraper --bin ingest -- overpass --pref 13   # POI of Tokyo
cargo run -p chintai-scraper --bin ingest -- overpass --pref 14   # POI of Kanagawa
cargo run -p chintai-scraper --bin ingest -- rentals --suumo tokyo/sc_setagaya --homes tokyo/setagaya-city
cargo run -p chintai-scraper --bin ingest -- geocode              # fill in the coordinates of the properties

# Front end (needs wasm-bindgen-cli and the wasm32 target)
web/build.sh --release          # -> web/dist

# Run
cargo run -p chintai-server     # http://127.0.0.1:3000
```

When `GOOGLE_MAPS_API_KEY` is set, the details of public transport routes (company, line, type, stations, times) are fetched from
the Google Routes API (without it, only the total time from the scrape is used).

Data sources: OpenStreetMap (Overpass / Nominatim / GSI elevation), SUUMO and LIFULL HOME'S,
route search by scraping Google Maps → (with a key) the Google Routes API. The map is MapLibre GL with OSM tiles.

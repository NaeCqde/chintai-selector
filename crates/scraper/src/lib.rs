//! chintai-scraper — data collection for chintai-selector.
//!
//! Everything here talks to the outside world with `reqwest` only (no browser).
//! Modules:
//!   - `http`    : the shared, per-host rate-limited HTTP client
//!   - `geocode` : building name / address -> { name, address, lat, lng }
//!   - `route`   : A -> B travel time, walk elevation delta, transit detail  (later)
//!   - `poi`     : nationwide convenience stores / supermarkets / parcel points (later)
//!   - `rental`  : SUUMO / LIFULL HOME'S listing adapters + de-duplication     (later)
//!
//! It may depend on `chintai-db` to persist what it collects, but never on the
//! server crate.

pub mod geocode;
pub mod http;
pub mod poi;
pub mod route;

pub use http::HttpClient;

/// Read `GOOGLE_MAPS_API_KEY` from the environment, if set and non-empty.
pub fn google_api_key() -> Option<String> {
    std::env::var("GOOGLE_MAPS_API_KEY")
        .ok()
        .filter(|s| !s.trim().is_empty())
}

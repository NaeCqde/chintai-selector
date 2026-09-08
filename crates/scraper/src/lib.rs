//! chintai-scraper — data collection for chintai-selector.
//!
//! Everything here talks to the outside world with `reqwest` only (no browser).
//! Modules:
//!   - `geocode`  : building name / address -> { name, address, lat, lng }
//!   - `route`    : A -> B travel time, walk elevation delta, transit spread
//!   - `poi`      : nationwide convenience stores / supermarkets / parcel points
//!   - `rental`   : SUUMO / LIFULL HOME'S listing adapters + de-duplication
//!
//! It may depend on `chintai-db` to persist what it collects, but never on the
//! server crate.

/// Shared HTTP client factory: sensible timeouts, gzip, a real User-Agent, and
/// (later) a polite rate limiter shared across adapters.
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(30))
        .user_agent(concat!(
            "chintai-selector/",
            env!("CARGO_PKG_VERSION"),
            " (+https://github.com/NaeCqde/chintai-selector)"
        ))
        .build()
        .unwrap_or_default()
}

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
pub mod pacing;
pub mod poi;
pub mod rental;
pub mod route;

pub use http::HttpClient;

/// Read `GOOGLE_MAPS_API_KEY` from the environment, if set and non-empty.
pub fn google_api_key() -> Option<String> {
    std::env::var("GOOGLE_MAPS_API_KEY")
        .ok()
        .filter(|s| !s.trim().is_empty())
}

/// Read `.env` into the environment: the one in the current directory, or, when there is none, the one next to the executable
/// (the one that is found is the only one that is read). Variables that are already set are kept.
pub fn load_env() {
    let cwd = std::path::PathBuf::from(".env");
    let path = if cwd.is_file() {
        cwd
    } else {
        match std::env::current_exe()
            .ok()
            .map(|e| e.canonicalize().unwrap_or(e))
            .and_then(|e| e.parent().map(|d| d.join(".env")))
        {
            Some(p) if p.is_file() => p,
            _ => return,
        }
    };
    let _ = dotenvy::from_path(path);
}

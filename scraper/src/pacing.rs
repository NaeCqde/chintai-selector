//! Per-host request pacing, in one place.
//!
//! Every adapter that talks to a specific site applies its own entry here to
//! its `HttpClient` (via [`HttpClient::set_host_interval`]) in its
//! constructor, so tuning one site's speed never touches any other site's.
//!
//! This is deliberately *not* auto-throttling: nothing here watches for 429s,
//! CAPTCHA pages, or error rates and backs off on its own. If a site starts
//! blocking us, the fix is to lower the relevant constant below by hand and
//! rebuild — a human decision, not a runtime heuristic.

use std::time::Duration;

/// suumo.jp — rental listings.
pub const SUUMO: Duration = Duration::from_millis(300);

/// www.homes.co.jp — rental listings (LIFULL HOME'S).
pub const HOMES: Duration = Duration::from_millis(300);

/// www.athome.co.jp — rental listings. AtHome runs a request-velocity bot
/// check (a puzzle-CAPTCHA page instead of listings) that a burst of rapid
/// manual testing tripped once during development. If a sustained crawl
/// starts coming back with 0 buildings across the board, that's this
/// kicking in again — lower this constant first.
pub const ATHOME: Duration = Duration::from_millis(300);

/// www.google.com — route scraping (`route::GoogleMapsScrape`), both the
/// bulk property->station precompute and the live per-search property->place
/// lookups the engine does at request time.
pub const GOOGLE_MAPS: Duration = Duration::from_millis(300);

/// nominatim.openstreetmap.org — geocoding fallback. Nominatim's own usage
/// policy asks for <= 1 req/s from one source; this runs faster than that on
/// request, for personal use.
pub const NOMINATIM: Duration = Duration::from_millis(300);

/// cyberjapandata2.gsi.go.jp — per-point walk-segment elevation lookups (up
/// to 16 requests per route).
pub const GSI_ELEVATION: Duration = Duration::from_millis(300);

/// The public Overpass API mirrors — nationwide POI collection. Each query
/// is a heavy, possibly minutes-long server-side computation, so this is
/// less "requests per second" and more "don't start a second heavy query
/// right on top of the last one."
pub const OVERPASS: Duration = Duration::from_millis(300);

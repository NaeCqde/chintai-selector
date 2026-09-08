//! `ingest` — the periodic data-refresh binary.
//!
//! Intended to be driven by cron / systemd-timer / Task Scheduler; it holds no
//! scheduler of its own. Subcommands (`brands`, `overpass`, `google-fill`,
//! `rentals <area>`, `all`) land alongside their scraper modules.

fn main() -> anyhow::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));
    log::info!("chintai ingest: no subcommands wired up yet");
    Ok(())
}

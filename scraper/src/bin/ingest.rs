//! `ingest` — the periodic data-refresh binary. Driven by cron / Task Scheduler;
//! it holds no scheduler of its own.
//!
//! Usage:
//!   ingest overpass --pref <1..47> [--grid <NxM>]
//!   ingest overpass --bbox <south,west,north,east>
//!   ingest overpass --all [--grid <NxM>]
//!
//! DB: `CHINTAI_DB` env (SeaORM URL) or `sqlite://chintai.sqlite?mode=rwc`.
//! `GOOGLE_MAPS_API_KEY` enables the Routes API fallback where used.

use chintai_scraper::poi::{prefectures, Overpass};
use chintai_scraper::HttpClient;

#[tokio::main]
async fn main() {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));
    if let Err(e) = run().await {
        log::error!("{e:#}");
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first().map(String::as_str) else {
        print_usage();
        std::process::exit(2);
    };

    let db_url =
        std::env::var("CHINTAI_DB").unwrap_or_else(|_| "sqlite://chintai.sqlite?mode=rwc".into());
    let db = chintai_db::connect(&db_url).await?;
    let http = HttpClient::default();

    match cmd {
        "overpass" => overpass_cmd(&db, &args[1..]).await,
        "rentals" => rentals_cmd(&db, &args[1..]).await,
        "geocode" => geocode_cmd(&db, &http, &args[1..]).await,
        "google-stations" => google_stations_cmd(&db, &args[1..]).await,
        "backfill-station-names" => backfill_station_names_cmd(&db, &args[1..]).await,
        "help" | "-h" | "--help" => {
            print_usage();
            Ok(())
        }
        other => {
            log::error!("unknown command: {other}");
            print_usage();
            std::process::exit(2);
        }
    }
}

async fn overpass_cmd(db: &chintai_db::DatabaseConnection, args: &[String]) -> anyhow::Result<()> {
    let opts = Opts::parse(args);
    let grid = opts.grid.unwrap_or((3, 3));
    let overpass = Overpass::new();

    let (task, targets): (String, Vec<u32>) = if opts.all {
        (
            "overpass:all".into(),
            prefectures::all().iter().map(|p| p.code).collect(),
        )
    } else if let Some(code) = opts.pref {
        (format!("overpass:{code}"), vec![code])
    } else if let Some(bbox) = opts.bbox {
        let run_id = chintai_db::ingest_run_start(db, "overpass:bbox").await?;
        let res = overpass.ingest_bbox(db, bbox).await;
        finish(
            db,
            run_id,
            &res.as_ref()
                .map(|c| c.total_pois())
                .map_err(|e| e.to_string()),
        );
        let c = res?;
        log::info!("done: {c:?}");
        return Ok(());
    } else {
        anyhow::bail!("overpass needs --pref <n>, --bbox <s,w,n,e>, or --all");
    };

    let run_id = chintai_db::ingest_run_start(db, &task).await?;
    let mut grand = chintai_scraper::poi::IngestCounts::default();
    let mut ok = true;
    for code in targets {
        match overpass.ingest_prefecture(db, code, grid).await {
            Ok(c) => {
                let name = prefectures::by_code(code).map(|p| p.name).unwrap_or("?");
                log::info!("{name}: {} pois", c.total_pois());
                grand = accumulate(grand, c);
            }
            Err(e) => {
                ok = false;
                log::error!("prefecture {code} failed: {e}");
            }
        }
    }
    log::info!("grand total: {grand:?}");
    let note = format!("{grand:?}");
    chintai_db::ingest_run_finish(db, run_id, ok, Some(&note)).await?;
    Ok(())
}

fn accumulate(
    mut a: chintai_scraper::poi::IngestCounts,
    b: chintai_scraper::poi::IngestCounts,
) -> chintai_scraper::poi::IngestCounts {
    a.convenience += b.convenience;
    a.supermarket += b.supermarket;
    a.parcel += b.parcel;
    a.post_office += b.post_office;
    a.station += b.station;
    a.skipped += b.skipped;
    a
}

fn finish(db: &chintai_db::DatabaseConnection, run_id: i32, res: &Result<u64, String>) {
    let (ok, note) = match res {
        Ok(n) => (true, format!("{n} pois")),
        Err(e) => (false, e.clone()),
    };
    let db = db.clone();
    tokio::spawn(async move {
        let _ = chintai_db::ingest_run_finish(&db, run_id, ok, Some(&note)).await;
    });
}

async fn rentals_cmd(db: &chintai_db::DatabaseConnection, args: &[String]) -> anyhow::Result<()> {
    use chintai_scraper::rental::{ingest_area, Athome, Homes, Suumo};

    let mut suumo_areas: Vec<String> = Vec::new();
    let mut homes_areas: Vec<String> = Vec::new();
    let mut athome_areas: Vec<String> = Vec::new();
    let mut pages = 3u32;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--suumo" => it.next().map(|s| suumo_areas.push(s.clone())).unwrap_or(()),
            "--homes" => it.next().map(|s| homes_areas.push(s.clone())).unwrap_or(()),
            "--athome" => it
                .next()
                .map(|s| athome_areas.push(s.clone()))
                .unwrap_or(()),
            "--pages" => pages = it.next().and_then(|s| s.parse().ok()).unwrap_or(pages),
            // Accepted for backward compatibility with older invocations;
            // per-site pace now lives in `chintai_scraper::pacing` and each
            // adapter applies its own entry, so this no longer does anything.
            "--interval-ms" => {
                it.next();
            }
            _ => {}
        }
    }
    if suumo_areas.is_empty() && homes_areas.is_empty() && athome_areas.is_empty() {
        anyhow::bail!("rentals needs --suumo/--homes/--athome <area> (repeatable). e.g. --suumo tokyo/sc_setagaya --homes tokyo/setagaya-city --athome tokyo/setagaya-city");
    }
    log::info!(
        "rentals: {} suumo + {} homes + {} athome areas, {} pages max (pace: see chintai_scraper::pacing)",
        suumo_areas.len(),
        homes_areas.len(),
        athome_areas.len(),
        pages,
    );

    for area in suumo_areas {
        let run = chintai_db::ingest_run_start(db, &format!("rentals:suumo:{area}")).await?;
        let src = Suumo::new(HttpClient::default());
        let res = ingest_area(db, &src, &area, pages).await;
        log_rentals("suumo", &area, &res);
        chintai_db::ingest_run_finish(db, run, res.is_ok(), Some(&format!("{res:?}"))).await?;
        if let Err(e) = res {
            log::error!("suumo {area}: {e}");
        }
    }
    for area in homes_areas {
        let run = chintai_db::ingest_run_start(db, &format!("rentals:homes:{area}")).await?;
        let src = Homes::new(HttpClient::browser(chintai_scraper::pacing::HOMES));
        let res = ingest_area(db, &src, &area, pages).await;
        log_rentals("homes", &area, &res);
        chintai_db::ingest_run_finish(db, run, res.is_ok(), Some(&format!("{res:?}"))).await?;
        if let Err(e) = res {
            log::error!("homes {area}: {e}");
        }
    }
    for area in athome_areas {
        let run = chintai_db::ingest_run_start(db, &format!("rentals:athome:{area}")).await?;
        let src = Athome::new(HttpClient::default());
        let res = ingest_area(db, &src, &area, pages).await;
        log_rentals("athome", &area, &res);
        chintai_db::ingest_run_finish(db, run, res.is_ok(), Some(&format!("{res:?}"))).await?;
        if let Err(e) = res {
            log::error!("athome {area}: {e}");
        }
    }
    Ok(())
}

fn log_rentals(
    site: &str,
    area: &str,
    res: &Result<chintai_scraper::rental::RentalCounts, chintai_scraper::rental::RentalError>,
) {
    match res {
        Ok(c) => log::info!(
            "{site} {area}: {} pages, {} buildings, {} units, {} listings",
            c.pages,
            c.buildings,
            c.units,
            c.listings
        ),
        Err(e) => log::error!("{site} {area}: {e}"),
    }
}

async fn geocode_cmd(
    db: &chintai_db::DatabaseConnection,
    http: &HttpClient,
    args: &[String],
) -> anyhow::Result<()> {
    use chintai_scraper::geocode::{Geocoder, NominatimGeocoder};

    let limit = args
        .iter()
        .position(|a| a == "--limit")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(200u64);

    let pending = chintai_db::properties_missing_coords(db, limit).await?;
    log::info!("geocoding {} properties", pending.len());
    let geocoder = NominatimGeocoder::new(http.clone());
    let run = chintai_db::ingest_run_start(db, "geocode").await?;
    let (mut ok, mut miss) = (0u64, 0u64);

    for p in pending {
        let query = if p.address.is_empty() {
            &p.name
        } else {
            &p.address
        };
        match geocoder.geocode(query).await {
            Ok(Some(hit)) => {
                chintai_db::set_property_coords(db, p.id, hit.lat, hit.lng).await?;
                ok += 1;
            }
            _ => miss += 1,
        }
    }
    log::info!("geocoded {ok}, missed {miss}");
    chintai_db::ingest_run_finish(db, run, true, Some(&format!("ok={ok} miss={miss}"))).await?;
    Ok(())
}

/// One-time backfill for properties upserted before `station_name` existed:
/// pull the station name straight out of an already-scraped listing's
/// `raw_json` (no network — the data is already in the DB). New properties
/// get `station_name` directly from `rental::ingest_area` and never need this.
async fn backfill_station_names_cmd(
    db: &chintai_db::DatabaseConnection,
    args: &[String],
) -> anyhow::Result<()> {
    let limit = args
        .iter()
        .position(|a| a == "--limit")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(5_000u64);

    let pending = chintai_db::properties_without_station_rows(db, limit).await?;
    log::info!(
        "backfill-station-names: {} properties pending",
        pending.len()
    );
    let (mut found, mut empty) = (0u64, 0u64);

    for p in &pending {
        let mut names: Vec<String> = Vec::new();
        'units: for u in chintai_db::units_for_property(db, p.id).await? {
            for l in chintai_db::listings_for_unit(db, u.id).await? {
                let from_this = stations_from_raw_json(&l.raw_json);
                if !from_this.is_empty() {
                    names = from_this;
                    break 'units;
                }
            }
        }
        if names.is_empty() {
            // Empty-name sentinel row: "checked, no station in any listing" —
            // so this property leaves `properties_without_station_rows`.
            chintai_db::upsert_property_station(db, p.id, "", 0).await?;
            empty += 1;
        } else {
            for (rank, n) in names.iter().enumerate() {
                chintai_db::upsert_property_station(db, p.id, n, rank as i32).await?;
            }
            found += 1;
        }
    }
    log::info!("backfill-station-names done: found={found} empty={empty}");
    Ok(())
}

/// All station names in a `NewListing::raw_json` blob
/// (`{"building":..,"stations":[{"line":..,"station":..,"walk_min":..}],..}`).
fn stations_from_raw_json(raw: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Vec::new();
    };
    v.pointer("/stations")
        .and_then(|s| s.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|s| s.pointer("/station").and_then(|x| x.as_str()))
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Precomputes the *listing's own* station leg: for every property with a
/// scraped station name but no walk time yet, resolve that station in our DB
/// and scrape the real Google Maps walking route to it. This is the only
/// route the project pre-saves in bulk — routes to a user's own registered
/// places are always computed live, at search time, by the engine.
async fn google_stations_cmd(
    db: &chintai_db::DatabaseConnection,
    args: &[String],
) -> anyhow::Result<()> {
    use chintai_scraper::route::{GoogleMapsScrape, GsiElevation, LatLng, Mode, RouteProvider};

    // One bounded pass, not a re-querying loop: `properties_needing_station_time`
    // still matches a row after a *transient* Google failure (we don't mark
    // those), so looping on the same query could spin forever re-fetching an
    // unchanging head of the queue. A single pass always makes bounded
    // progress; run the command again (see scripts/crawl_google_stations.sh)
    // to keep going — already-resolved and already-unresolved rows won't be
    // re-fetched.
    let limit = args
        .iter()
        .position(|a| a == "--limit")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(2_000u64);

    let http = HttpClient::default();
    let provider = GoogleMapsScrape::new(http.clone(), GsiElevation::new(http));
    let run = chintai_db::ingest_run_start(db, "google-stations").await?;
    let (mut ok, mut miss, mut unresolved) = (0u64, 0u64, 0u64);

    // Catch-up pass: properties whose walk leg resolved before car-time /
    // station-coordinate support existed (added after this crawl was
    // already running against ~150k properties — without this, everything
    // resolved so far would be permanently stuck missing car_min).
    let car_backfill = chintai_db::property_stations_needing_car_backfill(db, limit).await?;
    log::info!(
        "google-stations: {} station rows need car-time backfill",
        car_backfill.len()
    );
    for (ps, p) in &car_backfill {
        let name = &ps.station_name;
        let Some(station) = chintai_db::station_by_name(db, name).await? else {
            continue;
        };
        let origin = LatLng::new(p.lat.unwrap(), p.lng.unwrap());
        let dest = LatLng::new(station.lat, station.lng);
        let car_min = match provider.route(origin, dest, Mode::Car, None).await {
            Ok(car_route) => Some((car_route.duration.as_secs() as f64 / 60.0).ceil() as i32),
            Err(e) => {
                log::warn!("google-stations backfill: {} -> {name} (car): {e}", p.name);
                None
            }
        };
        chintai_db::set_property_station_car(db, ps.id, car_min, station.lat, station.lng).await?;
        ok += 1;
    }

    let pending = chintai_db::property_stations_needing_time(db, limit).await?;
    log::info!("google-stations: {} station rows pending", pending.len());

    for (ps, p) in &pending {
        let name = &ps.station_name;
        let Some(station) = chintai_db::station_by_name(db, name).await? else {
            chintai_db::mark_property_station_unresolved(db, ps.id).await?;
            unresolved += 1;
            continue;
        };
        let origin = LatLng::new(p.lat.unwrap(), p.lng.unwrap());
        let dest = LatLng::new(station.lat, station.lng);

        // Walk is required (it's what leaves the pending set); car (for
        // 送迎モード) is best-effort alongside it. If car fails while walk
        // succeeds, the row is still marked resolved with `car_min = None`
        // and won't be retried — a known gap, acceptable since both legs hit
        // the same reliable scrape path and a car-only failure should be
        // rare. A dedicated car-only backfill pass can be added later if
        // that turns out wrong.
        let walk = provider.route(origin, dest, Mode::Walk, None).await;
        match walk {
            Ok(walk_route) => {
                let walk_min = (walk_route.duration.as_secs() as f64 / 60.0).ceil() as i32;
                let car_min = match provider.route(origin, dest, Mode::Car, None).await {
                    Ok(car_route) => {
                        Some((car_route.duration.as_secs() as f64 / 60.0).ceil() as i32)
                    }
                    Err(e) => {
                        log::warn!("google-stations: {} -> {name} (car): {e}", p.name);
                        None
                    }
                };
                chintai_db::set_property_station_time(
                    db,
                    ps.id,
                    walk_min,
                    walk_route.distance_m,
                    car_min,
                    station.lat,
                    station.lng,
                )
                .await?;
                ok += 1;
            }
            Err(e) => {
                log::warn!("google-stations: {} -> {name} (walk): {e}", p.name);
                miss += 1;
            }
        }
        if (ok + miss + unresolved) % 50 == 0 {
            log::info!("google-stations: ok={ok} unresolved={unresolved} miss={miss}");
        }
    }

    log::info!("google-stations done: ok={ok} unresolved={unresolved} miss={miss}");
    chintai_db::ingest_run_finish(
        db,
        run,
        true,
        Some(&format!("ok={ok} unresolved={unresolved} miss={miss}")),
    )
    .await?;
    Ok(())
}

#[derive(Default)]
struct Opts {
    pref: Option<u32>,
    bbox: Option<(f64, f64, f64, f64)>,
    all: bool,
    grid: Option<(usize, usize)>,
}

impl Opts {
    fn parse(args: &[String]) -> Self {
        let mut o = Opts::default();
        let mut it = args.iter();
        while let Some(a) = it.next() {
            match a.as_str() {
                "--all" => o.all = true,
                "--pref" => o.pref = it.next().and_then(|s| s.parse().ok()),
                "--bbox" => {
                    o.bbox = it.next().and_then(|s| {
                        let n: Vec<f64> =
                            s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                        (n.len() == 4).then_some((n[0], n[1], n[2], n[3]))
                    })
                }
                "--grid" => {
                    o.grid = it.next().and_then(|s| {
                        let (x, y) = s.split_once('x')?;
                        Some((x.parse().ok()?, y.parse().ok()?))
                    })
                }
                _ => {}
            }
        }
        o
    }
}

fn print_usage() {
    eprintln!(
        "ingest <command>\n\n\
         overpass --pref <1..47> [--grid NxM]        one prefecture (default grid 3x3)\n\
         overpass --bbox <s,w,n,e>                   one bounding box\n\
         overpass --all [--grid NxM]                 every prefecture\n\
         rentals --suumo/--homes/--athome <area> [--pages N] (each repeatable)\n\
                                                    e.g. --suumo tokyo/sc_setagaya --homes tokyo/setagaya-city\n\
                                                         --athome tokyo/setagaya-city\n\
         geocode [--limit N]                        fill property coordinates via Nominatim\n\
         backfill-station-names [--limit N]          one-time: pull station_name from already-\n\
                                                    scraped listings (default 5000/run, no network)\n\
         google-stations [--limit N]                Google-Maps-scrape walk time to each\n\
                                                    property's listed station (default 2000/run)\n"
    );
}

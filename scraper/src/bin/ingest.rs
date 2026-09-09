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
    use chintai_scraper::rental::{ingest_area, Homes, Suumo};

    let mut suumo_areas: Vec<String> = Vec::new();
    let mut homes_areas: Vec<String> = Vec::new();
    let mut pages = 3u32;
    let mut interval_ms = 1000u64; // user asked for a gentle 1 req/s
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--suumo" => it.next().map(|s| suumo_areas.push(s.clone())).unwrap_or(()),
            "--homes" => it.next().map(|s| homes_areas.push(s.clone())).unwrap_or(()),
            "--pages" => pages = it.next().and_then(|s| s.parse().ok()).unwrap_or(pages),
            "--interval-ms" => {
                interval_ms = it
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(interval_ms)
            }
            _ => {}
        }
    }
    if suumo_areas.is_empty() && homes_areas.is_empty() {
        anyhow::bail!("rentals needs --suumo <area> and/or --homes <area> (repeatable). e.g. --suumo tokyo/sc_setagaya --homes tokyo/setagaya-city");
    }
    let iv = std::time::Duration::from_millis(interval_ms);
    log::info!(
        "rentals: {} suumo + {} homes areas, {} pages max, {}ms/req",
        suumo_areas.len(),
        homes_areas.len(),
        pages,
        interval_ms
    );

    for area in suumo_areas {
        let run = chintai_db::ingest_run_start(db, &format!("rentals:suumo:{area}")).await?;
        let src = Suumo::new(HttpClient::new(iv));
        let res = ingest_area(db, &src, &area, pages).await;
        log_rentals("suumo", &area, &res);
        chintai_db::ingest_run_finish(db, run, res.is_ok(), Some(&format!("{res:?}"))).await?;
        if let Err(e) = res {
            log::error!("suumo {area}: {e}");
        }
    }
    for area in homes_areas {
        let run = chintai_db::ingest_run_start(db, &format!("rentals:homes:{area}")).await?;
        let src = Homes::new(HttpClient::browser(iv));
        let res = ingest_area(db, &src, &area, pages).await;
        log_rentals("homes", &area, &res);
        chintai_db::ingest_run_finish(db, run, res.is_ok(), Some(&format!("{res:?}"))).await?;
        if let Err(e) = res {
            log::error!("homes {area}: {e}");
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

    let pending = chintai_db::properties_without_station_name(db, limit).await?;
    log::info!(
        "backfill-station-names: {} properties pending",
        pending.len()
    );
    let (mut found, mut empty) = (0u64, 0u64);

    for p in &pending {
        let mut name = None;
        'units: for u in chintai_db::units_for_property(db, p.id).await? {
            for l in chintai_db::listings_for_unit(db, u.id).await? {
                if let Some(n) = station_name_from_raw_json(&l.raw_json) {
                    name = Some(n);
                    break 'units;
                }
            }
        }
        match &name {
            Some(n) => {
                chintai_db::set_property_station_name(db, p.id, n).await?;
                found += 1;
            }
            // Empty-string sentinel: "checked, no station in any listing" —
            // distinct from NULL ("not checked yet") so this property isn't
            // re-selected by every future run.
            None => {
                chintai_db::set_property_station_name(db, p.id, "").await?;
                empty += 1;
            }
        }
    }
    log::info!("backfill-station-names done: found={found} empty={empty}");
    Ok(())
}

/// Pull `stations[0].station` out of a `NewListing::raw_json` blob
/// (`{"building":..,"address":..,"stations":[{"line":..,"station":..,"walk_min":..}],"unit":..}`).
fn station_name_from_raw_json(raw: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    v.pointer("/stations/0/station")
        .and_then(|s| s.as_str())
        .map(str::to_owned)
        .filter(|s| !s.is_empty())
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

    let pending = chintai_db::properties_needing_station_time(db, limit).await?;
    log::info!("google-stations: {} properties pending", pending.len());
    let run = chintai_db::ingest_run_start(db, "google-stations").await?;
    let (mut ok, mut miss, mut unresolved) = (0u64, 0u64, 0u64);

    for p in &pending {
        let Some(name) = &p.station_name else {
            continue;
        };
        let Some(station) = chintai_db::station_by_name(db, name).await? else {
            chintai_db::mark_property_station_unresolved(db, p.id).await?;
            unresolved += 1;
            continue;
        };
        let origin = LatLng::new(p.lat.unwrap(), p.lng.unwrap());
        let dest = LatLng::new(station.lat, station.lng);
        match provider.route(origin, dest, Mode::Walk).await {
            Ok(route) => {
                let mins = (route.duration.as_secs() as f64 / 60.0).ceil() as i32;
                chintai_db::set_property_station_time(db, p.id, mins, route.distance_m).await?;
                ok += 1;
            }
            Err(e) => {
                log::warn!("google-stations: {} -> {name}: {e}", p.name);
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
         rentals --suumo <area> --homes <area> [--pages N]\n\
                                                    e.g. --suumo tokyo/sc_setagaya --homes tokyo/setagaya-city\n\
         geocode [--limit N]                        fill property coordinates via Nominatim\n\
         backfill-station-names [--limit N]          one-time: pull station_name from already-\n\
                                                    scraped listings (default 5000/run, no network)\n\
         google-stations [--limit N]                Google-Maps-scrape walk time to each\n\
                                                    property's listed station (default 2000/run)\n"
    );
}

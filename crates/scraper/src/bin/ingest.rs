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
        "overpass" => overpass_cmd(&db, &http, &args[1..]).await,
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

async fn overpass_cmd(
    db: &chintai_db::DatabaseConnection,
    http: &HttpClient,
    args: &[String],
) -> anyhow::Result<()> {
    let opts = Opts::parse(args);
    let grid = opts.grid.unwrap_or((3, 3));
    let overpass = Overpass::new(http.clone());

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
         overpass --pref <1..47> [--grid NxM]   one prefecture (default grid 3x3)\n\
         overpass --bbox <s,w,n,e>              one bounding box\n\
         overpass --all [--grid NxM]            every prefecture\n"
    );
}

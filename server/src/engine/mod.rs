//! The filter + ranking engine.
//!
//! 1. Pull every unit + geocoded property from the DB and apply the static
//!    filters (rent, area, layout, building age) in memory.
//! 2. For each surviving property, find the nearest convenience store /
//!    supermarket / cheap supermarket / parcel point (straight-line) and drop
//!    it if a POI walk-distance filter fails.
//! 3. For each place, compute the real route (cached) and drop the property if
//!    any place is over its time limit.
//! 4. Normalise each ranking metric to 0..1, combine with the given weights,
//!    sort, and return the top N with full per-place route detail.

mod geo;
mod types;

pub use types::*;

use std::collections::HashMap;
use std::time::Duration;

use chintai_db::entities::enums::PriceTier;
use chintai_db::{entities, DatabaseConnection};
use chintai_scraper::route::{
    route_cached, DriveLeg, EstimateProvider, LatLng, Leg, Mode, RouteProvider, RouteResult,
};
use futures::stream::{self, StreamExt};

use geo::{haversine_m, walk_minutes};

/// Never route more than this many candidates in one search — keeps latency
/// bounded when a loose filter leaves hundreds of buildings. The closest ones
/// (crow-flies) are kept.
const ROUTE_CAP: usize = 60;

/// How far over a place's limit the *estimate* may be for a candidate to still
/// get a real route check. The estimate is rough (crow-flies × a fixed detour
/// factor), so hard-dropping anything the estimate puts even slightly over
/// would hide properties a real route would clear. The real check in step 3b
/// is the source of truth for what actually gets listed.
const ESTIMATE_SLACK: f64 = 1.35;

/// Optimistic door-to-door speed as metres per minute — a candidate further
/// than `minutes * this` from a place cannot possibly reach it in time, so it
/// is dropped before routing.
fn reachable_ceiling_m(mode: chintai_scraper::route::Mode, minutes: u32) -> f64 {
    use chintai_scraper::route::Mode;
    let per_min = match mode {
        Mode::Walk => 100.0,  // ~6 km/h
        Mode::Car => 1_000.0, // ~60 km/h
        // Car to the station is fast, so a bit more generous than plain
        // transit's ceiling.
        Mode::PickupTransit => 1_000.0,
        Mode::Transit => 900.0, // ~54 km/h incl. access/wait, generous
    };
    minutes as f64 * per_min + 400.0 // + slack for the last-leg walk
}

/// The listing's best resolved station (`property_station` row with the
/// smallest non-negative `walk_min`, from `ingest google-stations`).
fn nearest_station_of(ps: Option<&entities::property_station::Model>) -> Option<NearestStation> {
    let ps = ps?;
    let walk_minutes = ps.walk_min?;
    if walk_minutes < 0 {
        return None;
    }
    Some(NearestStation {
        name: ps.station_name.clone(),
        walk_minutes: walk_minutes as u32,
        distance_m: ps.distance_m.unwrap_or_default(),
    })
}

/// 送迎モード's fixed first leg: someone drives the property's precomputed
/// station car-time (`ingest google-stations`), never a live route call.
/// `None` until that batch resolved *both* legs for this property, or if the
/// car leg specifically failed (`car_min == -1` or `None`).
fn station_car_leg(ps: Option<&entities::property_station::Model>) -> Option<(u32, LatLng)> {
    let ps = ps?;
    let car_min = ps.car_min?;
    if car_min < 0 {
        return None;
    }
    let lat = ps.station_lat?;
    let lng = ps.station_lng?;
    Some((car_min as u32, LatLng::new(lat, lng)))
}

/// Splice 送迎モード's fixed car-to-station leg onto the front of a live
/// station->place transit result: bumps every duration field and prepends a
/// synthetic drive leg to each itinerary.
fn prepend_drive_leg(leg1_minutes: u32, mut leg2: RouteResult) -> RouteResult {
    let leg1_dur = Duration::from_secs(leg1_minutes as u64 * 60);
    leg2.mode = Mode::PickupTransit;
    leg2.duration += leg1_dur;
    leg2.transit_fastest = leg2.transit_fastest.map(|d| d + leg1_dur);
    leg2.transit_slowest = leg2.transit_slowest.map(|d| d + leg1_dur);
    for it in &mut leg2.itineraries {
        it.duration += leg1_dur;
        it.legs.insert(
            0,
            Leg::Drive(DriveLeg {
                duration: leg1_dur,
                distance_m: None,
                polyline: None,
            }),
        );
    }
    leg2
}

fn straight_line_sum(c: &Candidate, places: &[PlaceSpec]) -> f64 {
    let (lat, lng) = (c.prop.lat.unwrap(), c.prop.lng.unwrap());
    places
        .iter()
        .map(|p| haversine_m(lat, lng, p.lat, p.lng))
        .sum()
}

pub struct Engine<'a> {
    pub db: &'a DatabaseConnection,
    /// The accurate provider (Google Maps scrape). After the fast estimate
    /// pass ranks everything, this confirms the real travel time for every
    /// place of the top candidates — a property is only listed once every
    /// place is verified within its limit.
    pub provider: &'a dyn RouteProvider,
}

impl<'a> Engine<'a> {
    pub fn new(db: &'a DatabaseConnection, provider: &'a dyn RouteProvider) -> Self {
        Self { db, provider }
    }

    pub async fn search(&self, req: &SearchRequest) -> anyhow::Result<SearchResponse> {
        let ranking = effective_ranking(&req.filters, &req.ranking);
        let this_year = current_year();

        // --- 1. load + static filter -------------------------------------
        let units = chintai_db::units_with_geocoded_property(self.db).await?;
        let mut by_prop: HashMap<i32, (entities::property::Model, Vec<entities::unit::Model>)> =
            HashMap::new();
        for (u, p) in units {
            by_prop
                .entry(p.id)
                .or_insert_with(|| (p, Vec::new()))
                .1
                .push(u);
        }

        let stores = chintai_db::stores_with_brand(self.db).await?;
        let deliveries = chintai_db::deliveries_with_coords(self.db).await?;

        let mut candidates: Vec<Candidate> = Vec::new();
        for (_, (prop, units)) in by_prop {
            if let Some(max_age) = req.filters.max_built_age {
                if let Some(by) = prop.built_year {
                    if this_year - by > max_age {
                        continue;
                    }
                }
            }
            let matching: Vec<_> = units
                .into_iter()
                .filter(|u| unit_passes(u, &req.filters))
                .collect();
            if matching.is_empty() {
                continue;
            }

            let (plat, plng) = (prop.lat.unwrap(), prop.lng.unwrap());
            let near = NearbyPois::compute(plat, plng, &stores, &deliveries);

            if let Some(max) = req.filters.max_conveni_walk_min {
                if near.conveni_m.map(walk_minutes).is_none_or(|m| m > max) {
                    continue;
                }
            }
            if let Some(max) = req.filters.max_super_walk_min {
                let d = if req.filters.require_cheap_super {
                    near.cheap_super_m
                } else {
                    near.super_m
                };
                if d.map(walk_minutes).is_none_or(|m| m > max) {
                    continue;
                }
            }
            if let Some(max_m) = req.filters.max_delivery_m {
                if near.delivery_m.is_none_or(|m| m > max_m) {
                    continue;
                }
            }

            candidates.push(Candidate {
                prop,
                units: matching,
                near,
                station: None,
            });
        }
        let considered = candidates.len();

        // --- 1b. straight-line pre-filter ------------------------------------
        // Routing is the expensive step (a scrape / API call per candidate x
        // place). Drop anything that physically cannot reach a place inside its
        // limit even at an optimistic speed, then route only the closest
        // `ROUTE_CAP` by summed crow-flies distance.
        candidates.retain(|c| {
            let (plat, plng) = (c.prop.lat.unwrap(), c.prop.lng.unwrap());
            req.places.iter().all(|place| {
                haversine_m(plat, plng, place.lat, place.lng)
                    <= reachable_ceiling_m(place.mode, place.max_minutes)
            })
        });
        candidates.sort_by(|a, b| {
            straight_line_sum(a, &req.places)
                .partial_cmp(&straight_line_sum(b, &req.places))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let plausible = candidates.len();
        candidates.truncate(ROUTE_CAP);
        let capped = plausible > candidates.len();

        // Attach each survivor's best precomputed station (walk + car time).
        let ids: Vec<i32> = candidates.iter().map(|c| c.prop.id).collect();
        let mut station_map = chintai_db::resolved_stations_for(self.db, &ids).await?;
        for c in &mut candidates {
            c.station = station_map.remove(&c.prop.id);
        }

        // --- 2. estimate-route every candidate (instant, no network) -------
        let estimator = EstimateProvider;
        let routed: Vec<RoutedCandidate> = stream::iter(candidates.into_iter().map(|c| {
            let estimator = &estimator;
            async move {
                let origin = LatLng::new(c.prop.lat.unwrap(), c.prop.lng.unwrap());
                let mut places = Vec::with_capacity(req.places.len());
                let mut all_ok = true;
                for place in &req.places {
                    let dest = LatLng::new(place.lat, place.lng);
                    let route = if place.mode == Mode::PickupTransit {
                        match station_car_leg(c.station.as_ref()) {
                            Some((car_min, station)) => {
                                let leg2 = estimator
                                    .route(
                                        station,
                                        dest,
                                        Mode::Transit,
                                        Some(place.departure.plus_minutes(car_min as i64)),
                                    )
                                    .await
                                    .expect("estimate is infallible");
                                prepend_drive_leg(car_min, leg2)
                            }
                            // Car-to-station leg not precomputed yet for this
                            // property (see `station_car_leg`'s doc) — don't
                            // fabricate a number, just fail this place.
                            None => RouteResult::unresolvable(place.mode),
                        }
                    } else {
                        estimator
                            .route(origin, dest, place.mode, Some(place.departure))
                            .await
                            .expect("estimate is infallible")
                    };
                    let minutes = (route.duration.as_secs() as f64 / 60.0).ceil() as u32;
                    let within = minutes <= place.max_minutes;
                    // Lenient gate: keep anything the rough estimate puts
                    // within `ESTIMATE_SLACK` of the cap so step 3b can settle
                    // it with a real route. `within_limit` still records the
                    // strict estimate result.
                    all_ok &= minutes as f64 <= place.max_minutes as f64 * ESTIMATE_SLACK;
                    places.push(PlaceAccess {
                        label: place.label.clone(),
                        minutes,
                        within_limit: within,
                        route,
                    });
                }
                all_ok.then_some(RoutedCandidate { c, places })
            }
        }))
        .buffer_unordered(64)
        .filter_map(|x| async move { x })
        .collect()
        .await;

        // --- 3. score + sort ------------------------------------------------
        let metrics: Vec<Metrics> = routed.iter().map(Metrics::of).collect();
        let ranges = MetricRanges::of(&metrics);

        let mut scored: Vec<(f64, RoutedCandidate, Metrics)> = routed
            .into_iter()
            .zip(metrics)
            .map(|(rc, m)| {
                let score = ranking
                    .iter()
                    .map(|spec| spec.weight * ranges.goodness(spec, &m))
                    .sum();
                (score, rc, m)
            })
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        // --- 3b. verify with real Google Maps routes -----------------------
        // The estimate pass is optimistic (crow-flies × a detour factor), so
        // a candidate it let through can still be over the real limit —
        // especially for transit. Walk the ranked list, replace every place's
        // estimate with a real routed time + transfer detail, and keep the
        // property ONLY if every place is confirmed within its cap. An
        // unverified place (route fetch failed) is treated as not cleared.
        // Stop once `limit` properties are confirmed, or the pool runs out.
        let want = req.limit.max(1) as usize;
        let mut kept: Vec<(f64, RoutedCandidate)> = Vec::with_capacity(want);
        let mut verified = 0usize;
        for (score, mut rc, _) in scored {
            if kept.len() >= want {
                break;
            }
            verified += 1;
            let origin = LatLng::new(rc.c.prop.lat.unwrap(), rc.c.prop.lng.unwrap());
            let mut all_cleared = true;
            for (pa, place) in rc.places.iter_mut().zip(&req.places) {
                let dest = LatLng::new(place.lat, place.lng);

                let real = if place.mode == Mode::PickupTransit {
                    // Leg 1 (property -> station, car) is fixed, precomputed
                    // data — never a live call. Leg 2 (station -> place,
                    // transit) is refined live like any other place, just
                    // from the station instead of the property, departing
                    // after the drive.
                    match station_car_leg(rc.c.station.as_ref()) {
                        Some((car_min, station)) => route_cached(
                            self.db,
                            self.provider,
                            station,
                            dest,
                            Mode::Transit,
                            Some(place.departure.plus_minutes(car_min as i64)),
                        )
                        .await
                        .map(|leg2| prepend_drive_leg(car_min, leg2)),
                        None => Ok(RouteResult::unresolvable(place.mode)),
                    }
                } else {
                    route_cached(
                        self.db,
                        self.provider,
                        origin,
                        dest,
                        place.mode,
                        Some(place.departure),
                    )
                    .await
                };

                match real {
                    Ok(real) => {
                        let minutes = (real.duration.as_secs() as f64 / 60.0).ceil() as u32;
                        pa.minutes = minutes;
                        pa.within_limit = minutes <= place.max_minutes;
                        pa.route = real;
                        all_cleared &= pa.within_limit;
                    }
                    // Couldn't get a real route — don't guess. An unverified
                    // place is not a cleared place, so this property is out.
                    Err(e) => {
                        log::debug!("verify route failed for place {:?}: {e}", place.label);
                        pa.within_limit = false;
                        all_cleared = false;
                    }
                }
            }
            if all_cleared {
                kept.push((score, rc));
            }
        }

        let matched = kept.len();

        // --- 4. materialise ------------------------------------------------
        let mut results = Vec::with_capacity(kept.len());
        for (i, (score, rc)) in kept.into_iter().enumerate() {
            results.push(self.to_ranked(i as u32 + 1, score, rc).await?);
        }

        Ok(SearchResponse {
            results,
            considered,
            plausible,
            routed: verified,
            capped,
            matched,
        })
    }

    async fn to_ranked(
        &self,
        rank: u32,
        score: f64,
        rc: RoutedCandidate,
    ) -> anyhow::Result<RankedProperty> {
        let RoutedCandidate { c, places } = rc;
        let nearest_station = nearest_station_of(c.station.as_ref());
        let mut units = c.units;
        units.sort_by_key(|u| u.rent_yen.unwrap_or(i32::MAX));

        let mut views = Vec::with_capacity(units.len());
        for u in &units {
            let sources = chintai_db::listings_for_unit(self.db, u.id)
                .await?
                .into_iter()
                .map(|l| UnitSource {
                    site: format!("{:?}", l.site).to_lowercase(),
                    url: l.url,
                })
                .collect();
            views.push(unit_view(u, sources));
        }
        let best_unit = views.remove(0);

        Ok(RankedProperty {
            rank,
            score,
            property_id: c.prop.id,
            name: c.prop.name,
            address: c.prop.address,
            lat: c.prop.lat.unwrap(),
            lng: c.prop.lng.unwrap(),
            built_year: c.prop.built_year,
            structure: c.prop.structure,
            best_unit,
            other_units: views,
            places,
            nearest_station,
            nearest_conveni_m: c.near.conveni_m,
            nearest_super_m: c.near.super_m,
            nearest_cheap_super_m: c.near.cheap_super_m,
            nearest_delivery_m: c.near.delivery_m,
        })
    }
}

// --------------------------------------------------------------------------

struct Candidate {
    prop: entities::property::Model,
    units: Vec<entities::unit::Model>,
    near: NearbyPois,
    /// Best resolved `property_station` row (filled after the pre-filter, for
    /// the candidates that survive it).
    station: Option<entities::property_station::Model>,
}

struct RoutedCandidate {
    c: Candidate,
    places: Vec<PlaceAccess>,
}

#[derive(Default)]
struct NearbyPois {
    conveni_m: Option<f64>,
    super_m: Option<f64>,
    cheap_super_m: Option<f64>,
    delivery_m: Option<f64>,
}

impl NearbyPois {
    fn compute(
        lat: f64,
        lng: f64,
        stores: &[(entities::store::Model, entities::brand::Model)],
        deliveries: &[entities::delivery::Model],
    ) -> Self {
        use chintai_db::entities::enums::BrandKind;
        let mut n = NearbyPois::default();
        for (s, b) in stores {
            let d = haversine_m(lat, lng, s.lat.unwrap(), s.lng.unwrap());
            match b.kind {
                BrandKind::ConvenienceStore => min_into(&mut n.conveni_m, d),
                BrandKind::Supermarket => {
                    min_into(&mut n.super_m, d);
                    if b.price_tier == PriceTier::Cheap {
                        min_into(&mut n.cheap_super_m, d);
                    }
                }
            }
        }
        for de in deliveries {
            let d = haversine_m(lat, lng, de.lat.unwrap(), de.lng.unwrap());
            min_into(&mut n.delivery_m, d);
        }
        n
    }
}

fn min_into(slot: &mut Option<f64>, v: f64) {
    *slot = Some(slot.map_or(v, |cur| cur.min(v)));
}

fn unit_passes(u: &entities::unit::Model, f: &Filters) -> bool {
    if let Some(max) = f.max_rent_yen {
        if u.rent_yen.is_none_or(|r| r > max) {
            return false;
        }
    }
    if let Some(min) = f.min_area_m2 {
        if u.area_m2.is_none_or(|a| a < min) {
            return false;
        }
    }
    if !f.layouts.is_empty() {
        match &u.layout {
            Some(l) if f.layouts.iter().any(|w| w.eq_ignore_ascii_case(l)) => {}
            _ => return false,
        }
    }
    true
}

fn unit_view(u: &entities::unit::Model, sources: Vec<UnitSource>) -> UnitView {
    UnitView {
        unit_id: u.id,
        floor: u.floor.clone(),
        layout: u.layout.clone(),
        area_m2: u.area_m2,
        rent_yen: u.rent_yen,
        admin_fee_yen: u.admin_fee_yen,
        deposit_yen: u.deposit_yen,
        key_money_yen: u.key_money_yen,
        sources,
    }
}

fn effective_ranking(_f: &Filters, given: &[RankSpec]) -> Vec<RankSpec> {
    if given.is_empty() {
        vec![
            RankSpec::new(RankKey::AccessTotal, 1.0),
            RankSpec::new(RankKey::Rent, 0.5),
        ]
    } else {
        given.to_vec()
    }
}

fn current_year() -> i32 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    1970 + (secs / 31_556_952) as i32
}

// --- scoring --------------------------------------------------------------

const N_METRICS: usize = 8;

fn metric_idx(k: RankKey) -> usize {
    match k {
        RankKey::AccessTotal => 0,
        RankKey::AccessWorst => 1,
        RankKey::Rent => 2,
        RankKey::Area => 3,
        RankKey::RentPerArea => 4,
        RankKey::NearestSuper => 5,
        RankKey::NearestConveni => 6,
        RankKey::BuildYear => 7,
    }
}

/// One candidate's raw metric values, indexed by [`metric_idx`].
#[derive(Clone, Copy)]
struct Metrics([f64; N_METRICS]);

impl Metrics {
    fn of(rc: &RoutedCandidate) -> Self {
        let mins: Vec<f64> = rc.places.iter().map(|p| p.minutes as f64).collect();
        let cheapest =
            rc.c.units
                .iter()
                .filter_map(|u| u.rent_yen)
                .min()
                .unwrap_or(0) as f64;
        let area =
            rc.c.units
                .iter()
                .filter_map(|u| u.area_m2)
                .fold(0.0_f64, f64::max);
        let mut v = [0.0; N_METRICS];
        v[0] = mins.iter().sum();
        v[1] = mins.iter().cloned().fold(0.0, f64::max);
        v[2] = cheapest;
        v[3] = area;
        v[4] = if area > 0.0 {
            cheapest / area
        } else {
            cheapest
        };
        v[5] =
            rc.c.near
                .cheap_super_m
                .or(rc.c.near.super_m)
                .unwrap_or(5_000.0);
        v[6] = rc.c.near.conveni_m.unwrap_or(3_000.0);
        v[7] = rc.c.prop.built_year.unwrap_or(1970) as f64;
        Metrics(v)
    }
}

struct MetricRanges {
    lo: [f64; N_METRICS],
    hi: [f64; N_METRICS],
}

impl MetricRanges {
    fn of(all: &[Metrics]) -> Self {
        let mut lo = [f64::INFINITY; N_METRICS];
        let mut hi = [f64::NEG_INFINITY; N_METRICS];
        for m in all {
            for i in 0..N_METRICS {
                lo[i] = lo[i].min(m.0[i]);
                hi[i] = hi[i].max(m.0[i]);
            }
        }
        Self { lo, hi }
    }

    /// 0..1 where 1 = best, honouring `spec.ascending`.
    fn goodness(&self, spec: &RankSpec, m: &Metrics) -> f64 {
        let i = metric_idx(spec.key);
        let span = self.hi[i] - self.lo[i];
        let norm = if span.abs() < f64::EPSILON {
            0.5
        } else {
            (m.0[i] - self.lo[i]) / span
        };
        if spec.ascending {
            1.0 - norm
        } else {
            norm
        }
    }
}

#[cfg(test)]
mod tests;

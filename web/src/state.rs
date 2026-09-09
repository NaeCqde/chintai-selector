//! Client-side app state: the editable form model plus the last search result.
//! Persisted to `localStorage` so a reload keeps your places and filters.

use chintai_types::{
    DepartureTime, Filters, Mode, PlaceKind, PlaceSpec, RankKey, RankSpec, SearchRequest,
    SearchResponse,
};
use gloo_storage::{LocalStorage, Storage};
use leptos::prelude::*;
use serde::{Deserialize, Serialize};

const STORE_KEY: &str = "chintai.form.v1";

/// A destination as edited in the form (before geocoding resolves coordinates).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaceInput {
    pub label: String,
    pub query: String,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub mode: Mode,
    pub max_minutes: u32,
    pub kind: PlaceKind,
    /// Local departure time-of-day for this place's route search. Defaults
    /// from `kind` when it's picked (see `PlaceRow`'s kind selector) but
    /// stays independently editable afterwards.
    pub departure_hour: u8,
    pub departure_minute: u8,
    /// Transient: last geocode error, not persisted.
    #[serde(skip)]
    pub error: Option<String>,
}

impl Default for PlaceInput {
    fn default() -> Self {
        let d = PlaceKind::Commute.default_departure();
        Self {
            label: "職場".into(),
            query: String::new(),
            lat: None,
            lng: None,
            mode: Mode::Transit,
            max_minutes: 30,
            kind: PlaceKind::Commute,
            departure_hour: d.hour,
            departure_minute: d.minute,
            error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankRow {
    pub key: RankKey,
    pub weight: f64,
    pub enabled: bool,
    pub ascending: bool,
}

pub fn default_ranking() -> Vec<RankRow> {
    [
        (RankKey::AccessTotal, 1.0, true),
        (RankKey::Rent, 0.6, true),
        (RankKey::Area, 0.4, false),
        (RankKey::NearestSuper, 0.3, false),
        (RankKey::NearestConveni, 0.2, false),
        (RankKey::BuildYear, 0.2, false),
        (RankKey::AccessWorst, 0.0, false),
        (RankKey::RentPerArea, 0.0, false),
    ]
    .into_iter()
    .map(|(key, weight, enabled)| RankRow {
        key,
        weight,
        enabled,
        ascending: RankSpec::new(key, weight).ascending,
    })
    .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormModel {
    pub places: Vec<PlaceInput>,
    pub filters: Filters,
    pub ranking: Vec<RankRow>,
    pub limit: u32,
    #[serde(default)]
    pub refine: bool,
}

impl Default for FormModel {
    fn default() -> Self {
        Self {
            places: vec![PlaceInput::default()],
            filters: Filters::default(),
            ranking: default_ranking(),
            limit: 50,
            refine: false,
        }
    }
}

impl FormModel {
    pub fn load() -> Self {
        LocalStorage::get(STORE_KEY).unwrap_or_default()
    }

    pub fn save(&self) {
        let _ = LocalStorage::set(STORE_KEY, self);
    }

    /// Build the API request from the resolved places. `Err` lists why it can't
    /// (no geocoded place yet).
    pub fn to_request(&self) -> Result<SearchRequest, String> {
        let places: Vec<PlaceSpec> = self
            .places
            .iter()
            .filter_map(|p| {
                Some(PlaceSpec {
                    label: p.label.clone(),
                    lat: p.lat?,
                    lng: p.lng?,
                    mode: p.mode,
                    max_minutes: p.max_minutes,
                    kind: p.kind,
                    departure: DepartureTime::new(p.departure_hour, p.departure_minute),
                })
            })
            .collect();
        if places.is_empty() {
            return Err("目的地を1つ以上ジオコーディングしてください".into());
        }
        let ranking: Vec<RankSpec> = self
            .ranking
            .iter()
            .filter(|r| r.enabled && r.weight > 0.0)
            .map(|r| RankSpec {
                key: r.key,
                weight: r.weight,
                ascending: r.ascending,
            })
            .collect();
        Ok(SearchRequest {
            places,
            filters: self.filters.clone(),
            ranking,
            limit: self.limit.max(1),
            refine: self.refine,
        })
    }
}

/// The reactive handles shared through context.
#[derive(Clone, Copy)]
pub struct AppState {
    pub form: RwSignal<FormModel>,
    pub results: RwSignal<Option<SearchResponse>>,
    pub loading: RwSignal<bool>,
    pub error: RwSignal<Option<String>>,
    pub selected: RwSignal<Option<i32>>,
}

impl AppState {
    pub fn new() -> Self {
        let form = RwSignal::new(FormModel::load());
        Effect::new(move |_| {
            form.with(|f| f.save());
        });
        Self {
            form,
            results: RwSignal::new(None),
            loading: RwSignal::new(false),
            error: RwSignal::new(None),
            selected: RwSignal::new(None),
        }
    }
}

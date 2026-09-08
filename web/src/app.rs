//! The whole UI. PC: left settings/results panel, right map. Mobile: map with a
//! bottom sheet (CSS only). Buggy-but-complete is fine for now.

use chintai_types::{Leg, Mode, PoiKind, RankKey, RouteResult};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::maplibre;
use crate::net;
use crate::state::{AppState, PlaceInput};

#[component]
pub fn App() -> impl IntoView {
    let state = AppState::new();
    provide_context(state);

    view! {
        <div class="layout">
            <aside class="panel">
                <header class="brand">
                    <h1>"chintai-selector"</h1>
                    <p>"通う場所から住まいを選ぶ"</p>
                </header>
                <PlaceEditor/>
                <FilterPanel/>
                <RankingControls/>
                <SearchBar/>
                <ResultList/>
            </aside>
            <main class="map-pane">
                <MapPane/>
            </main>
        </div>
    }
}

// --------------------------------------------------------------------------

#[component]
fn PlaceEditor() -> impl IntoView {
    let state = expect_context::<AppState>();
    let form = state.form;

    let add = move |_| {
        form.update(|f| f.places.push(PlaceInput::default()));
    };

    view! {
        <section class="card">
            <h2>"通う場所"</h2>
            {move || (0..form.with(|f| f.places.len()))
                .map(|i| view! { <PlaceRow idx=i/> })
                .collect_view()}
            <button class="ghost" on:click=add>"+ 場所を追加"</button>
        </section>
    }
}

#[component]
fn PlaceRow(idx: usize) -> impl IntoView {
    let state = expect_context::<AppState>();
    let form = state.form;

    let get = move || form.with(|f| f.places.get(idx).cloned());
    let set_field = move |mutate: Box<dyn Fn(&mut PlaceInput)>| {
        form.update(|f| {
            if let Some(p) = f.places.get_mut(idx) {
                mutate(p);
            }
        });
    };

    let remove = move |_| {
        form.update(|f| {
            if f.places.len() > 1 {
                f.places.remove(idx);
            }
        });
    };

    let do_geocode = move |_| {
        let q = get().map(|p| p.query).unwrap_or_default();
        if q.trim().is_empty() {
            return;
        }
        spawn_local(async move {
            match net::geocode(&q).await {
                Ok(Some(hit)) => form.update(|f| {
                    if let Some(p) = f.places.get_mut(idx) {
                        p.lat = Some(hit.lat);
                        p.lng = Some(hit.lng);
                        p.error = None;
                        if p.label.is_empty() {
                            p.label = hit.name.clone();
                        }
                    }
                }),
                Ok(None) => form.update(|f| {
                    if let Some(p) = f.places.get_mut(idx) {
                        p.error = Some("見つかりませんでした".into());
                        p.lat = None;
                        p.lng = None;
                    }
                }),
                Err(e) => form.update(|f| {
                    if let Some(p) = f.places.get_mut(idx) {
                        p.error = Some(e);
                    }
                }),
            }
        });
    };

    view! {
        <div class="place-row">
            <div class="row">
                <input
                    class="label-in"
                    placeholder="ラベル (職場など)"
                    prop:value=move || get().map(|p| p.label).unwrap_or_default()
                    on:input=move |ev| {
                        let v = event_target_value(&ev);
                        set_field(Box::new(move |p| p.label = v.clone()));
                    }
                />
                <button class="icon" on:click=remove title="削除">"✕"</button>
            </div>
            <div class="row">
                <input
                    class="grow"
                    placeholder="住所 / 建物名 / 駅名"
                    prop:value=move || get().map(|p| p.query).unwrap_or_default()
                    on:input=move |ev| {
                        let v = event_target_value(&ev);
                        set_field(Box::new(move |p| p.query = v.clone()));
                    }
                    on:keydown=move |ev| if ev.key() == "Enter" { do_geocode(()) }
                />
                <button class="ghost" on:click=move |_| do_geocode(())>"検索"</button>
            </div>
            <div class="row small">
                <select
                    on:change=move |ev| {
                        let m = match event_target_value(&ev).as_str() {
                            "walk" => Mode::Walk,
                            "car" => Mode::Car,
                            _ => Mode::Transit,
                        };
                        set_field(Box::new(move |p| p.mode = m));
                    }
                    prop:value=move || match get().map(|p| p.mode) {
                        Some(Mode::Walk) => "walk",
                        Some(Mode::Car) => "car",
                        _ => "transit",
                    }
                >
                    <option value="walk">"完全徒歩"</option>
                    <option value="car">"車"</option>
                    <option value="transit">"公共交通＋徒歩"</option>
                </select>
                <label class="minutes">
                    <input
                        type="number"
                        min="1"
                        max="180"
                        prop:value=move || get().map(|p| p.max_minutes).unwrap_or(30)
                        on:input=move |ev| {
                            if let Ok(v) = event_target_value(&ev).parse::<u32>() {
                                set_field(Box::new(move |p| p.max_minutes = v));
                            }
                        }
                    />
                    "分以内"
                </label>
                <span class="geo-state">
                    {move || match get() {
                        Some(p) if p.lat.is_some() => view! { <span class="ok">"座標OK"</span> }.into_any(),
                        Some(p) => match p.error {
                            Some(e) => view! { <span class="err">{e}</span> }.into_any(),
                            None => view! { <span class="muted">"未解決"</span> }.into_any(),
                        },
                        None => ().into_any(),
                    }}
                </span>
            </div>
        </div>
    }
}

// --------------------------------------------------------------------------

#[component]
fn FilterPanel() -> impl IntoView {
    let state = expect_context::<AppState>();
    let form = state.form;

    let num_opt = move |get: fn(&chintai_types::Filters) -> Option<i32>,
                        set: fn(&mut chintai_types::Filters, Option<i32>)| {
        (
            move || {
                form.with(|f| get(&f.filters))
                    .map(|v| v.to_string())
                    .unwrap_or_default()
            },
            move |ev: leptos::ev::Event| {
                let v = event_target_value(&ev);
                let parsed = v.trim().parse::<i32>().ok();
                form.update(|f| set(&mut f.filters, parsed));
            },
        )
    };
    let (rent_v, rent_i) = num_opt(|f| f.max_rent_yen, |f, v| f.max_rent_yen = v);
    let (age_v, age_i) = num_opt(|f| f.max_built_age, |f, v| f.max_built_age = v);

    let layout_toggle = move |name: &'static str| {
        form.update(|f| {
            if let Some(pos) = f.filters.layouts.iter().position(|x| x == name) {
                f.filters.layouts.remove(pos);
            } else {
                f.filters.layouts.push(name.into());
            }
        });
    };
    let has_layout =
        move |name: &'static str| form.with(|f| f.filters.layouts.iter().any(|x| x == name));

    view! {
        <section class="card">
            <h2>"条件で絞り込む"</h2>
            <label class="field">
                "家賃上限 (円)"
                <input type="number" step="5000" prop:value=rent_v on:input=rent_i/>
            </label>
            <label class="field">
                "面積下限 (㎡)"
                <input
                    type="number" step="1"
                    prop:value=move || form.with(|f| f.filters.min_area_m2).map(|v| v.to_string()).unwrap_or_default()
                    on:input=move |ev| {
                        let p = event_target_value(&ev).trim().parse::<f64>().ok();
                        form.update(|f| f.filters.min_area_m2 = p);
                    }
                />
            </label>
            <label class="field">
                "築年数上限"
                <input type="number" step="1" prop:value=age_v on:input=age_i/>
            </label>
            <div class="field">
                "間取り"
                <div class="chips">
                    {["1R", "1K", "1DK", "1LDK", "2K", "2DK", "2LDK", "3LDK"].into_iter().map(|name| view! {
                        <button
                            class="chip"
                            class:on=move || has_layout(name)
                            on:click=move |_| layout_toggle(name)
                        >{name}</button>
                    }).collect_view()}
                </div>
            </div>
            <label class="field">
                "コンビニまで徒歩 (分)"
                <input
                    type="number" step="1"
                    prop:value=move || form.with(|f| f.filters.max_conveni_walk_min).map(|v| v.to_string()).unwrap_or_default()
                    on:input=move |ev| {
                        let p = event_target_value(&ev).trim().parse::<u32>().ok();
                        form.update(|f| f.filters.max_conveni_walk_min = p);
                    }
                />
            </label>
            <label class="field">
                "スーパーまで徒歩 (分)"
                <input
                    type="number" step="1"
                    prop:value=move || form.with(|f| f.filters.max_super_walk_min).map(|v| v.to_string()).unwrap_or_default()
                    on:input=move |ev| {
                        let p = event_target_value(&ev).trim().parse::<u32>().ok();
                        form.update(|f| f.filters.max_super_walk_min = p);
                    }
                />
            </label>
            <label class="checkbox">
                <input
                    type="checkbox"
                    prop:checked=move || form.with(|f| f.filters.require_cheap_super)
                    on:change=move |ev| {
                        let c = event_target_checked(&ev);
                        form.update(|f| f.filters.require_cheap_super = c);
                    }
                />
                "安い系スーパーを必須にする"
            </label>
            <label class="field">
                "荷物受取所まで (m)"
                <input
                    type="number" step="100"
                    prop:value=move || form.with(|f| f.filters.max_delivery_m).map(|v| v.to_string()).unwrap_or_default()
                    on:input=move |ev| {
                        let p = event_target_value(&ev).trim().parse::<f64>().ok();
                        form.update(|f| f.filters.max_delivery_m = p);
                    }
                />
            </label>
        </section>
    }
}

// --------------------------------------------------------------------------

fn rank_label(k: RankKey) -> &'static str {
    match k {
        RankKey::AccessTotal => "アクセス合計時間",
        RankKey::AccessWorst => "一番遠い場所の時間",
        RankKey::Rent => "家賃",
        RankKey::Area => "広さ",
        RankKey::RentPerArea => "㎡単価",
        RankKey::NearestSuper => "スーパーの近さ",
        RankKey::NearestConveni => "コンビニの近さ",
        RankKey::BuildYear => "築年の新しさ",
    }
}

#[component]
fn RankingControls() -> impl IntoView {
    let state = expect_context::<AppState>();
    let form = state.form;

    view! {
        <section class="card">
            <h2>"順位付けの重み"</h2>
            {move || (0..form.with(|f| f.ranking.len())).map(|i| {
                    let row = move || form.with(|f| f.ranking.get(i).cloned());
                    view! {
                        <div class="rank-row">
                            <label class="checkbox tight">
                                <input
                                    type="checkbox"
                                    prop:checked=move || row().map(|r| r.enabled).unwrap_or(false)
                                    on:change=move |ev| {
                                        let c = event_target_checked(&ev);
                                        form.update(|f| if let Some(r) = f.ranking.get_mut(i) { r.enabled = c });
                                    }
                                />
                                {move || row().map(|r| rank_label(r.key)).unwrap_or("")}
                            </label>
                            <input
                                type="range" min="0" max="1" step="0.1"
                                prop:value=move || row().map(|r| r.weight).unwrap_or(0.0)
                                on:input=move |ev| {
                                    if let Ok(w) = event_target_value(&ev).parse::<f64>() {
                                        form.update(|f| if let Some(r) = f.ranking.get_mut(i) { r.weight = w });
                                    }
                                }
                            />
                            <span class="w">{move || format!("{:.1}", row().map(|r| r.weight).unwrap_or(0.0))}</span>
                        </div>
                    }
                }).collect_view()}
        </section>
    }
}

// --------------------------------------------------------------------------

#[component]
fn SearchBar() -> impl IntoView {
    let state = expect_context::<AppState>();

    let run = move |_| {
        let req = match state.form.with(|f| f.to_request()) {
            Ok(r) => r,
            Err(e) => {
                state.error.set(Some(e));
                return;
            }
        };
        state.error.set(None);
        state.loading.set(true);
        spawn_local(async move {
            match net::search(&req).await {
                Ok(resp) => {
                    state.results.set(Some(resp));
                    state.error.set(None);
                }
                Err(e) => state.error.set(Some(e)),
            }
            state.loading.set(false);
        });
    };

    let form = state.form;
    view! {
        <div class="search-bar">
            <button class="primary" on:click=run disabled=move || state.loading.get()>
                {move || if state.loading.get() { "検索中…" } else { "物件を検索" }}
            </button>
            <label class="checkbox">
                <input
                    type="checkbox"
                    prop:checked=move || form.with(|f| f.refine)
                    on:change=move |ev| {
                        let c = event_target_checked(&ev);
                        form.update(|f| f.refine = c);
                    }
                />
                "上位の経路を正確に取得（乗換案内・数十秒）"
            </label>
            {move || state.error.get().map(|e| view! { <p class="err">{e}</p> })}
            {move || state.results.get().map(|r| view! {
                <p class="muted">
                    {format!(
                        "{} 件が条件突破 / {} 件を評価{}",
                        r.matched, r.routed.max(r.considered),
                        if r.capped { "（近い順に上限あり）" } else { "" }
                    )}
                </p>
            })}
        </div>
    }
}

// --------------------------------------------------------------------------

#[component]
fn ResultList() -> impl IntoView {
    let state = expect_context::<AppState>();

    view! {
        <section class="results">
            {move || state.results.get().map(|resp| {
                resp.results.into_iter().map(|r| view! { <ResultCard r=r/> }).collect_view()
            })}
        </section>
    }
}

#[component]
fn ResultCard(r: chintai_types::RankedProperty) -> impl IntoView {
    let state = expect_context::<AppState>();
    let pid = r.property_id;
    let selected = move || state.selected.get() == Some(pid);

    let yen = |v: Option<i32>| {
        v.map(|n| format!("{}万", (n as f64 / 10000.0)))
            .unwrap_or_else(|| "-".into())
    };
    let u = r.best_unit.clone();
    let places = r.places.clone();

    view! {
        <article class="result" class:sel=selected on:click=move |_| state.selected.set(Some(pid))>
            <div class="result-head">
                <span class="rank">{r.rank}</span>
                <div class="grow">
                    <h3>{r.name.clone()}</h3>
                    <p class="muted">{r.address.clone()}
                        {r.built_year.map(|y| format!(" ・ 築{}年", chintai_year().saturating_sub(y)))}
                    </p>
                </div>
                <span class="score" title="重み付けスコア">{format!("{:.2}", r.score)}</span>
            </div>

            <div class="unit">
                <strong>{yen(u.rent_yen)}</strong>
                <span>{u.layout.clone().unwrap_or_default()}</span>
                <span>{u.area_m2.map(|a| format!("{a:.1}㎡")).unwrap_or_default()}</span>
                <span>{u.floor.clone().unwrap_or_default()}</span>
                {(!r.other_units.is_empty()).then(|| view! {
                    <span class="muted">{format!("他 {} 部屋", r.other_units.len())}</span>
                })}
            </div>
            <div class="sources">
                {u.sources.iter().map(|s| {
                    let (site, url) = (s.site.clone(), s.url.clone());
                    view! { <a href=url target="_blank" rel="noreferrer">{site}</a> }
                }).collect_view()}
            </div>

            <ul class="places">
                {places.into_iter().map(|pa| view! {
                    <li class:over=!pa.within_limit>
                        <span class="pl-label">{pa.label.clone()}</span>
                        <span class="pl-min">{pa.minutes}"分"</span>
                        <span class="pl-mode">{pa.route.mode.label_ja()}</span>
                        <RouteDetail route=pa.route/>
                    </li>
                }).collect_view()}
            </ul>

            <div class="poi-near">
                {near_chip("コンビニ", r.nearest_conveni_m)}
                {near_chip("スーパー", r.nearest_super_m)}
                {near_chip("安いスーパー", r.nearest_cheap_super_m)}
                {near_chip("荷物", r.nearest_delivery_m)}
            </div>
        </article>
    }
}

fn near_chip(label: &str, m: Option<f64>) -> impl IntoView {
    let label = label.to_string();
    view! {
        <span class="near">
            {label} " " {m.map(|d| format!("{:.0}m (徒歩{}分)", d, (d / 80.0).ceil() as i32)).unwrap_or_else(|| "なし".into())}
        </span>
    }
}

#[component]
fn RouteDetail(route: RouteResult) -> impl IntoView {
    let open = RwSignal::new(false);
    let it = route.itineraries.first().cloned();
    let has_detail = it.as_ref().map(|i| !i.legs.is_empty()).unwrap_or(false);
    let is_est = route.provider.is_estimate();

    let spread = match (route.transit_fastest, route.transit_slowest) {
        (Some(f), Some(s)) if f != s => Some(format!(
            "最速{}分〜最遅{}分",
            f.as_secs() / 60,
            s.as_secs() / 60
        )),
        _ => None,
    };
    let elev = match (route.walk_ascent_m, route.walk_descent_m) {
        (Some(a), Some(d)) if a + d > 1.0 => Some(format!("↑{a:.0}m ↓{d:.0}m",)),
        _ => None,
    };

    view! {
        <div class="route-detail">
            {(has_detail).then(|| view! {
                <button class="link" on:click=move |ev| { ev.stop_propagation(); open.update(|o| *o = !*o); }>
                    {move || if open.get() { "▲ 経路" } else { "▼ 経路" }}
                </button>
            })}
            {is_est.then(|| view! { <span class="muted"> " 推定"</span> })}
            {spread.map(|s| view! { <span class="muted"> " " {s}</span> })}
            {elev.map(|e| view! { <span class="muted"> " " {e}</span> })}
            {move || (open.get()).then(|| {
                it.clone().map(|i| view! {
                    <ol class="legs">
                        {i.depart.clone().map(|d| view! { <li class="time">{format!("出発 {d}")}</li> })}
                        {i.legs.iter().map(|leg| view! { <li>{leg_line(leg)}</li> }).collect_view()}
                        {i.arrive.clone().map(|a| view! { <li class="time">{format!("到着 {a}")}</li> })}
                        {(i.transfers > 0).then(|| view! { <li class="muted">{format!("乗換 {} 回", i.transfers)}</li> })}
                    </ol>
                })
            })}
        </div>
    }
}

fn leg_line(leg: &Leg) -> String {
    match leg {
        Leg::Walk(w) => format!(
            "徒歩 {}分{}",
            w.duration.as_secs() / 60,
            w.distance_m
                .map(|d| format!(" ({d:.0}m)"))
                .unwrap_or_default()
        ),
        Leg::Drive(d) => format!("車 {}分", d.duration.as_secs() / 60),
        Leg::Transit(t) => {
            let op = t
                .operator
                .clone()
                .map(|o| format!("{o} "))
                .unwrap_or_default();
            let ty = t
                .train_type
                .clone()
                .map(|x| format!("[{x}] "))
                .unwrap_or_default();
            let stops = t.num_stops.map(|n| format!(" {n}駅")).unwrap_or_default();
            format!(
                "{op}{}{} {} → {}{}  {}分",
                ty,
                t.line_name,
                t.from_station,
                t.to_station,
                stops,
                t.duration.as_secs() / 60
            )
        }
    }
}

fn chintai_year() -> i32 {
    // Rough: good enough for a "築N年" label in the browser.
    2026
}

// --------------------------------------------------------------------------

#[component]
fn MapPane() -> impl IntoView {
    let state = expect_context::<AppState>();

    // Init once the element is in the DOM.
    Effect::new(move |_| {
        maplibre::init("map", 35.681, 139.767, 11.0);
    });

    // Push markers + route lines + POIs whenever results / selection change.
    Effect::new(move |_| {
        let results = state.results.get();
        let selected = state.selected.get();
        let places = state.form.with(|f| f.places.clone());

        let mut markers = Vec::new();
        let mut pts: Vec<[f64; 2]> = Vec::new();

        for p in &places {
            if let (Some(lat), Some(lng)) = (p.lat, p.lng) {
                markers.push(serde_json::json!({
                    "lat": lat, "lng": lng, "kind": "place", "label": p.label
                }));
                pts.push([lng, lat]);
            }
        }

        let mut lines = Vec::new();
        if let Some(resp) = &results {
            for r in &resp.results {
                let is_sel = selected == Some(r.property_id);
                markers.push(serde_json::json!({
                    "lat": r.lat, "lng": r.lng, "kind": "property",
                    "label": r.name, "rank": r.rank, "selected": is_sel
                }));
                pts.push([r.lng, r.lat]);
                if is_sel {
                    for (p, pa) in places.iter().zip(&r.places) {
                        if let (Some(plat), Some(plng)) = (p.lat, p.lng) {
                            lines.push(serde_json::json!({
                                "from": [r.lng, r.lat], "to": [plng, plat],
                                "color": mode_color(pa.route.mode)
                            }));
                        }
                    }
                }
            }
        }

        maplibre::set_markers(&serde_json::Value::Array(markers).to_string());
        maplibre::set_lines(&serde_json::Value::Array(lines).to_string());
        if !pts.is_empty() {
            maplibre::fit_to(&serde_json::to_string(&pts).unwrap_or_default());
        }

        // POIs for the current view, best-effort.
        if maplibre::ready() {
            let b = maplibre::bounds();
            if let Ok(bbox) = serde_json::from_str::<[f64; 4]>(&b) {
                spawn_local(async move {
                    if let Ok(pois) =
                        net::pois(bbox, "convenience,supermarket,delivery,station").await
                    {
                        let arr: Vec<_> = pois
                            .into_iter()
                            .map(|p| serde_json::json!({
                                "lat": p.lat, "lng": p.lng, "kind": poi_kind(p.kind), "label": p.name
                            }))
                            .collect();
                        maplibre::set_markers_pois(&serde_json::Value::Array(arr).to_string());
                    }
                });
            }
        }
    });

    view! { <div id="map"></div> }
}

fn mode_color(m: Mode) -> &'static str {
    match m {
        Mode::Walk => "#2e7d32",
        Mode::Car => "#c62828",
        Mode::Transit => "#1565c0",
    }
}

fn poi_kind(k: PoiKind) -> &'static str {
    match k {
        PoiKind::Convenience => "conveni",
        PoiKind::Supermarket => "super",
        PoiKind::CheapSupermarket => "super-cheap",
        PoiKind::Delivery => "delivery",
        PoiKind::Station => "station",
    }
}

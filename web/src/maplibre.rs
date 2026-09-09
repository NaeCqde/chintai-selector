//! Thin bindings to the MapLibre GL shim defined in `index.html` (which loads
//! `maplibre-gl` from a CDN). The shim keeps one map instance and exposes a
//! handful of imperative calls; Leptos effects drive it from signal changes.

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    /// Create the map in `#<container_id>`, centred on lat/lng at `zoom`.
    #[wasm_bindgen(js_namespace = chintaiMap, js_name = init)]
    pub fn init(container_id: &str, lat: f64, lng: f64, zoom: f64);

    /// Replace all overlay markers (places + properties). `markers_json` is a
    /// JSON array of `{lat,lng,kind,label,rank?,selected?}`.
    #[wasm_bindgen(js_namespace = chintaiMap, js_name = setMarkers)]
    pub fn set_markers(markers_json: &str);

    /// Replace the POI marker layer (convenience/super/delivery/station).
    #[wasm_bindgen(js_namespace = chintaiMap, js_name = setPois)]
    pub fn set_markers_pois(pois_json: &str);

    /// Draw route lines from a property to each destination. `lines_json` is a
    /// JSON array of `{from:[lng,lat], to:[lng,lat], color}`.
    #[wasm_bindgen(js_namespace = chintaiMap, js_name = setLines)]
    pub fn set_lines(lines_json: &str);

    /// Fit the viewport to `[[lng,lat],…]` (JSON).
    #[wasm_bindgen(js_namespace = chintaiMap, js_name = fitTo)]
    pub fn fit_to(points_json: &str);

    /// Current map bounds as `[south,west,north,east]` (JSON string), or "" if
    /// the map isn't ready.
    #[wasm_bindgen(js_namespace = chintaiMap, js_name = bounds)]
    pub fn bounds() -> String;

    /// True once the style has loaded.
    #[wasm_bindgen(js_namespace = chintaiMap, js_name = ready)]
    pub fn ready() -> bool;

    /// Show/hide a 国土地理院 hazard-map overlay. `kind` is one of "flood",
    /// "sediment" (急傾斜地/土石流/地すべり combined), "tsunami", "hightide".
    /// Layers are added lazily on first use, semi-transparent, under the POI
    /// dots and route lines.
    #[wasm_bindgen(js_namespace = chintaiMap, js_name = setHazard)]
    pub fn set_hazard(kind: &str, visible: bool);
}

//! API calls to `chintai-server`.

use chintai_types::{Geocoded, PoiPoint, SearchRequest, SearchResponse};
use gloo_net::http::Request;

pub async fn search(req: &SearchRequest) -> Result<SearchResponse, String> {
    Request::post("/api/search")
        .json(req)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json::<SearchResponse>()
        .await
        .map_err(|e| e.to_string())
}

pub async fn geocode(q: &str) -> Result<Option<Geocoded>, String> {
    Request::get("/api/geocode")
        .query([("q", q)])
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json::<Option<Geocoded>>()
        .await
        .map_err(|e| e.to_string())
}

pub async fn pois(bbox: [f64; 4], kinds: &str) -> Result<Vec<PoiPoint>, String> {
    let bbox = format!("{},{},{},{}", bbox[0], bbox[1], bbox[2], bbox[3]);
    Request::get("/api/pois")
        .query([("bbox", bbox.as_str()), ("kinds", kinds)])
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json::<Vec<PoiPoint>>()
        .await
        .map_err(|e| e.to_string())
}

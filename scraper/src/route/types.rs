//! Route data types live in `chintai-types` (shared with the frontend). Only
//! `RouteError` stays here — it wraps `reqwest::Error` via [`HttpError`].

pub use chintai_types::route::*;

use crate::http::HttpError;

#[derive(Debug, thiserror::Error)]
pub enum RouteError {
    #[error(transparent)]
    Http(#[from] HttpError),
    #[error("no route found")]
    NoRoute,
    #[error("response parse: {0}")]
    Parse(String),
    #[error("provider unavailable: {0}")]
    Unavailable(String),
}

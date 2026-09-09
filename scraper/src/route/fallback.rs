//! Try each provider in order; use the first *sufficient* answer.
//!
//! "Sufficient" is mode-aware: for `Walk`/`Car` any plausible duration is fine,
//! so the scrape usually wins. For `Transit` we want the structured leg detail
//! (operator / line / stations / times) the UI shows, so a scrape result that
//! is "a duration but no transit legs" is treated as insufficient and the next
//! provider (the API) is tried; the scrape totals are kept as a fallback if the
//! API also fails.

use super::types::*;
use super::RouteProvider;

pub struct FallbackProvider {
    providers: Vec<Box<dyn RouteProvider>>,
}

impl FallbackProvider {
    pub fn new(providers: Vec<Box<dyn RouteProvider>>) -> Self {
        Self { providers }
    }
}

fn is_sufficient(r: &RouteResult) -> bool {
    if r.duration.as_secs() == 0 {
        return false;
    }
    if r.mode == Mode::Transit {
        return r
            .itineraries
            .iter()
            .any(|it| it.legs.iter().any(|l| matches!(l, Leg::Transit(_))));
    }
    true
}

#[async_trait::async_trait]
impl RouteProvider for FallbackProvider {
    async fn route(
        &self,
        from: LatLng,
        to: LatLng,
        mode: Mode,
        departure: Option<DepartureTime>,
    ) -> Result<RouteResult, RouteError> {
        let mut best_partial: Option<RouteResult> = None;
        let mut last_err: Option<RouteError> = None;

        for p in &self.providers {
            match p.route(from, to, mode, departure).await {
                Ok(r) if is_sufficient(&r) => return Ok(r),
                Ok(r) => {
                    log::info!(
                        "route provider gave a partial {:?} result ({}s, no leg detail); trying next",
                        mode,
                        r.duration.as_secs()
                    );
                    best_partial = Some(r);
                }
                Err(e) => {
                    log::warn!("route provider failed: {e}; trying next");
                    last_err = Some(e);
                }
            }
        }

        best_partial
            .ok_or(())
            .or(Err(last_err.unwrap_or(RouteError::NoRoute)))
    }
}

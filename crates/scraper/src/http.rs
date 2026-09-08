//! One `reqwest::Client` for the whole crate, wrapped in a per-host rate
//! limiter so no single site sees a burst from us. Adapters call
//! [`HttpClient::get`] / [`HttpClient::get_json`] and never touch `reqwest`
//! directly.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tokio::time::Instant;

/// Default minimum gap between two requests to the same host.
const DEFAULT_MIN_INTERVAL: Duration = Duration::from_millis(1200);

#[derive(Clone)]
pub struct HttpClient {
    inner: reqwest::Client,
    limiter: Arc<RateLimiter>,
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new(DEFAULT_MIN_INTERVAL)
    }
}

impl HttpClient {
    pub fn new(min_interval: Duration) -> Self {
        let inner = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(40))
            .gzip(true)
            .user_agent(concat!(
                "chintai-selector/",
                env!("CARGO_PKG_VERSION"),
                " (personal use; +https://github.com/NaeCqde/chintai-selector)"
            ))
            .build()
            .expect("reqwest client");
        Self {
            inner,
            limiter: Arc::new(RateLimiter::new(min_interval)),
        }
    }

    /// Override the minimum interval for one host (e.g. Nominatim wants ≥ 1 s,
    /// a brand's own store list can usually go a little faster).
    pub fn set_host_interval(&self, host: &str, interval: Duration) {
        self.limiter.set_host_interval(host, interval);
    }

    pub fn raw(&self) -> &reqwest::Client {
        &self.inner
    }

    /// Rate-limited GET. `query` pairs are appended to the URL.
    pub async fn get(
        &self,
        url: &str,
        query: &[(&str, &str)],
    ) -> Result<reqwest::Response, HttpError> {
        let parsed = url::Url::parse(url).map_err(|e| HttpError::BadUrl(e.to_string()))?;
        let host = parsed.host_str().unwrap_or_default().to_owned();
        self.limiter.acquire(&host).await;

        let mut req = self.inner.get(parsed);
        if !query.is_empty() {
            req = req.query(query);
        }
        let resp = req.send().await?;
        Ok(resp)
    }

    /// GET + `error_for_status` + read body as text.
    pub async fn get_text(&self, url: &str, query: &[(&str, &str)]) -> Result<String, HttpError> {
        let resp = self.get(url, query).await?.error_for_status()?;
        Ok(resp.text().await?)
    }

    /// GET + `error_for_status` + parse JSON.
    pub async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        query: &[(&str, &str)],
    ) -> Result<T, HttpError> {
        let resp = self.get(url, query).await?.error_for_status()?;
        Ok(resp.json::<T>().await?)
    }

    /// Rate-limited POST of a JSON body with extra headers, parsing a JSON
    /// response. Used for the Google Routes API.
    pub async fn post_json<B, T>(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &B,
    ) -> Result<T, HttpError>
    where
        B: serde::Serialize + ?Sized,
        T: serde::de::DeserializeOwned,
    {
        let parsed = url::Url::parse(url).map_err(|e| HttpError::BadUrl(e.to_string()))?;
        let host = parsed.host_str().unwrap_or_default().to_owned();
        self.limiter.acquire(&host).await;

        let mut req = self.inner.post(parsed).json(body);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let resp = req.send().await?.error_for_status()?;
        Ok(resp.json::<T>().await?)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("bad url: {0}")]
    BadUrl(String),
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
}

// --------------------------------------------------------------------------

struct RateLimiter {
    default_interval: Duration,
    /// host -> (custom interval override, next allowed instant).
    hosts: Mutex<HashMap<String, HostSlot>>,
}

struct HostSlot {
    interval: Duration,
    next_allowed: Instant,
}

impl RateLimiter {
    fn new(default_interval: Duration) -> Self {
        Self {
            default_interval,
            hosts: Mutex::new(HashMap::new()),
        }
    }

    fn set_host_interval(&self, host: &str, interval: Duration) {
        // Best-effort synchronous poke; if the lock is held we just skip and the
        // caller can retry — overrides are set at startup, not in hot paths.
        if let Ok(mut map) = self.hosts.try_lock() {
            let slot = map.entry(host.to_owned()).or_insert(HostSlot {
                interval,
                next_allowed: Instant::now(),
            });
            slot.interval = interval;
        }
    }

    /// Block until it is this host's turn, then reserve the next slot.
    async fn acquire(&self, host: &str) {
        let wait_until = {
            let mut map = self.hosts.lock().await;
            let now = Instant::now();
            let slot = map.entry(host.to_owned()).or_insert(HostSlot {
                interval: self.default_interval,
                next_allowed: now,
            });
            let at = slot.next_allowed.max(now);
            slot.next_allowed = at + slot.interval;
            at
        };
        tokio::time::sleep_until(wait_until).await;
    }
}

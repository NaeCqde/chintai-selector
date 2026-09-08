//! chintai-types — pure serde data types shared by the scraper, the server
//! engine, and the WASM frontend. No dependency beyond `serde`; must stay
//! compilable to `wasm32-unknown-unknown`.

pub mod map;
pub mod route;
pub mod search;

pub use map::*;
pub use route::*;
pub use search::*;

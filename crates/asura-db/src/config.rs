//! The database URL, at the path every caller still uses. The implementation moved to `asura_config`
//! in cut 2e, because the crate named for configuration is where a setting belongs; this module is a
//! door for callers that have not moved, and it disappears with this crate in phase 6.

pub use asura_config::database::{database_url, DATABASE_URL_VARIABLE};
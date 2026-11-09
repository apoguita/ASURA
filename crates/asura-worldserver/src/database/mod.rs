//! The database layer: the pool, the schema check, and the access the stores need.
//!
//! The reference keeps `database/DatabaseEnv` (pools per logical database) and `database/Updater`
//! (the SQL updater) apart from both the game and the shared library. This module is that position for
//! this project: the program asks here for a pool and for the schema to be current, and it asks the
//! stores (`asura-store`) for facts - never for a query of its own.

use asura_store::PgPool;

/// The database URL, from the environment then the repository's `.env`.
///
/// One rule, stated once: `asura_config::database` owns it (`docs/reference/decisions` D33's spirit -
/// one home for a setting). This is a re-export so the program does not grow a second reader.
pub use asura_config::database::{database_url, DATABASE_URL_VARIABLE};

/// Open the program's pool.
///
/// The pool belongs to the store crate because the stores own the connections they ask through; this
/// is the program's door to it.
pub async fn connect(url: &str, max_connections: u32) -> sqlx::Result<PgPool> {
    asura_store::placement::connect(url, max_connections).await
}

/// Refuse to serve on a schema older than the code.
///
/// The reference's updater applies what is missing; this project refuses instead, because a world that
/// runs against a half-applied schema answers questions with columns that do not exist. The migration
/// runner still lives with the schema (`asura-db`), and this is the program's call into it.
pub async fn ensure_schema_is_current(pool: &PgPool) -> Result<(), String> {
    asura_db::migrations::ensure_current(pool)
        .await
        .map_err(|err| err.to_string())
}

//! The stores: the single place that holds durable state, one interface per owner.
//!
//! This crate is written fresh for the re-engineering (`docs/work/re-engineering-plan.md`, phase 2),
//! not carried over from the crate it replaces. The rule it is built on is one sentence: **a store
//! owns its tables and answers questions; it never decides anything and never reaches into another
//! store.**
//!
//! Three stores, one per kind of fact, taken from AzerothCore's split rather than from the old tree:
//!
//! * [`identity`] - who a client is: the account row and the portal pair it presents.
//! * `state` - what happened to whom: characters, items, bags, unlocks. Arrives with its own cut.
//! * `placement` - where a player is served: worlds, pools, leases, transfers. Arrives with its own cut.
//!
//! Until those cuts land, the old crate remains the implementation and this one holds only what has
//! moved; every caller moves with its own commit, so nothing is ever half-migrated.

pub mod catalog;
pub mod identity;
pub mod placement;
pub mod state;

/// The pool the stores ask their questions through. Callers build it once and lend it.
pub use sqlx::PgConnection;
pub use sqlx::PgPool;

/// The types a caller needs to name a row, re-exported so no caller depends on `sqlx` or `uuid`.
pub use uuid::Uuid;

/// Why a store could not answer.
///
/// A store owns its failures the same way it owns its tables: a caller gets one error type, not the
/// driver's, so swapping the driver is not a change to every caller.
#[derive(Debug)]
pub enum Error {
    /// The database refused the question, or could not be reached.
    Database(sqlx::Error),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::Database(err) => write!(f, "the store could not answer: {err}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<sqlx::Error> for Error {
    fn from(err: sqlx::Error) -> Self {
        Error::Database(err)
    }
}

/// The result every store method answers with.
pub type Result<T> = std::result::Result<T, Error>;

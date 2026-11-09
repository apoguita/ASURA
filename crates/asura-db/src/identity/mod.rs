//! The identity store: who a client is, and what it may present as itself.
//!
//! Two tables and one question. `t_account` holds the login identity; `t_session` holds one row per
//! conversation, keyed by the portal pair the client presents. Both belong to this store, and no other
//! store touches them.
//!
//! This is the first of the three stores the re-engineering plan splits out
//! (`docs/work/re-engineering-plan.md`, phase 2). It owns identity only: a character's rows, a world's
//! capacity and a map's content are other stores' facts, and this module must not reach for them.
//!
//! The two modules are re-exported at their old paths (`asura_db::account`, `asura_db::portal`) so no
//! caller changes until a later cut moves it; the old path is a door, not a second implementation.

pub mod account;
pub mod portal;

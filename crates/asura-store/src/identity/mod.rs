//! The identity store: who a client is, and no other fact.
//!
//! Two tables and one interface. `t_account` holds the login identity; `t_session` holds one row per
//! conversation, keyed by the portal pair the client presents. Both belong here, and no other store
//! touches them.
//!
//! What this store must never do, because the plan's phase 2 exists to stop it: read a character's
//! rows, a world's capacity, a map's content, or decide what an answer means. It answers questions
//! about identity and nothing else.

pub mod account;
pub mod portal;
pub mod session;

use crate::{PgPool, Result, Uuid};

/// The identity store, opened over the pool the caller owns.
///
/// A struct rather than free functions, because a store is one interface: a caller holds it, and a
/// later cut can give it its own credentials or its own database without changing any caller.
pub struct Identity<'a> {
    pool: &'a PgPool,
}

impl<'a> Identity<'a> {
    /// Open the store over a pool.
    pub fn new(pool: &'a PgPool) -> Self {
        Identity { pool }
    }

    /// The account row, or `None` when no such account exists.
    pub async fn account(&self, id: Uuid) -> Result<Option<account::Account>> {
        account::load(self.pool, id).await
    }

    /// Record the EULA version an account accepted; `false` means no row matched.
    pub async fn record_eula(&self, id: Uuid, version: i16) -> Result<bool> {
        account::record_eula(self.pool, id, version).await
    }

    /// The live portal pair an email resolves to, or `None` when that account has no open session.
    pub async fn portal_for_email(&self, email: &str) -> Result<Option<portal::PortalIdentity>> {
        portal::for_email(self.pool, email).await
    }

    /// Validate the pair a client presented, or `None` when it does not resolve.
    pub async fn session(
        &self,
        portal_user: Uuid,
        session: Uuid,
    ) -> Result<Option<session::Session>> {
        session::find(self.pool, portal_user, session).await
    }

    /// Close one session, so it can never resolve again.
    pub async fn close_session(&self, session: Uuid) -> Result<bool> {
        session::close(self.pool, session).await
    }

    /// Close every live session of an account except `keep`.
    pub async fn close_other_sessions(&self, account: Uuid, keep: Uuid) -> Result<u64> {
        session::close_others(self.pool, account, keep).await
    }
}

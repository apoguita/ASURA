//! The session: the pair a client presents, and the rule that a closed one never resolves.
//!
//! The client sends two UUIDs - a portal user id and a session id - and gets either a character list
//! or an error (`docs/reference/client-protocol.md` section 4). Two rules are easy to get subtly wrong
//! and both are enforced here, in one statement each:
//!
//! * **The pair is validated, never half-trusted.** One row must match both ids. A right portal user
//!   with a wrong session id is not "close enough".
//! * **A closed session is never honoured.** The newest session for an account wins and older ones are
//!   closed (`docs/research/gw2-service-design-lessons.md`, rule 1), so a closed row must not resolve.
//!
//! The SQL is the old crate's, unchanged: this cut moves the home of the fact, not the fact.

use crate::{PgPool, Result, Uuid};

/// A session that resolved: who the client is, and the account behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Session {
    /// The session id as presented and confirmed.
    pub session: Uuid,
    /// The account the pair belongs to, which is what everything else is keyed by.
    pub account: Uuid,
    /// The character this session last selected, if any.
    pub character: Option<Uuid>,
}

/// Validate the pair the client presented and resolve it to an account.
///
/// `Ok(None)` means the pair does not resolve **or** resolved in the past and is closed. The caller
/// must treat `None` as "refuse, with the client's auth error" - inventing a session here would be a
/// login bypass.
pub async fn find(pool: &PgPool, portal_user: Uuid, session: Uuid) -> Result<Option<Session>> {
    let row = sqlx::query_as::<_, (Uuid, Uuid, Option<Uuid>)>(
        "SELECT id, account_id, char_id
           FROM t_session
          WHERE portal_user = $1
            AND id = $2
            AND ended_at IS NULL",
    )
    .bind(portal_user)
    .bind(session)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(session, account, character)| Session {
        session,
        account,
        character,
    }))
}

/// Close a session, so it can never resolve again. Idempotent: closing a closed session is a no-op,
/// which is what makes it safe on every disconnect and again on a stale sweep.
pub async fn close(pool: &PgPool, session: Uuid) -> Result<bool> {
    let done = sqlx::query(
        "UPDATE t_session SET ended_at = now()
          WHERE id = $1 AND ended_at IS NULL",
    )
    .bind(session)
    .execute(pool)
    .await?;
    Ok(done.rows_affected() > 0)
}

/// Close every live session of an account except the one just validated.
///
/// This is "the newest session wins" expressed as SQL: when a player logs in again, the older sessions
/// are closed rather than left resolvable. One statement, one place, so it cannot be half-implemented
/// in a service.
pub async fn close_others(pool: &PgPool, account: Uuid, keep: Uuid) -> Result<u64> {
    let done = sqlx::query(
        "UPDATE t_session SET ended_at = now()
          WHERE account_id = $1
            AND id <> $2
            AND ended_at IS NULL",
    )
    .bind(account)
    .bind(keep)
    .execute(pool)
    .await?;
    Ok(done.rows_affected())
}

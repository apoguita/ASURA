//! The account row: the identity every other row is keyed by, and its own state.
//!
//! `eula_accepted` is the version of the EULA this account accepted, and `0` is "never". The client
//! shows the page while the number in `AUTH_SMSG_ACCOUNT_INFO` is lower than the version it knows, and
//! `AUTH_CMSG_ACCEPT_EULA` carries the version a person accepted
//! (`docs/reference/captures/login-session-2026-10-26.md`, `crates/asura-db/migrations/0012_account_eula.sql`).
//!
//! The SQL is the same SQL the old crate ran, deliberately: this store is a new home for the same
//! fact, and a moved query that changed its result would be a behaviour change, which phase 2 does
//! not carry. The interface around it is what changed.

use crate::{PgPool, Result, Uuid};

/// One account row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    /// The row's own identifier.
    pub id: Uuid,
    /// The account name the client presents.
    pub username: String,
    /// The EULA version this account accepted; `0` for never.
    pub eula_accepted: i16,
}

/// Read one account. `None` means no such row, which the login path treats as a refusal.
pub async fn load(pool: &PgPool, id: Uuid) -> Result<Option<Account>> {
    let row = sqlx::query_as::<_, (Uuid, String, i16)>(
        "SELECT id, username, eula_accepted FROM t_account WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(id, username, eula_accepted)| Account {
        id,
        username,
        eula_accepted,
    }))
}

/// Record the EULA version an account accepted. `false` means no row matched, which the caller can
/// only report: the client has already been answered either way.
pub async fn record_eula(pool: &PgPool, id: Uuid, version: i16) -> Result<bool> {
    let done = sqlx::query("UPDATE t_account SET eula_accepted = $2 WHERE id = $1")
        .bind(id)
        .bind(version)
        .execute(pool)
        .await?;
    Ok(done.rows_affected() == 1)
}

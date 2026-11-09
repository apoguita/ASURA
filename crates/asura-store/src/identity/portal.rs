//! The portal identity: which live session an email resolves to.
//!
//! The client does not ask an account database who it is; it asks the **portal web gate**
//! (`/Spawned/WebGate/...`) and then presents the pair it was given - the portal user id and the
//! session id (`docs/reference/client-protocol.md` #4,
//! `docs/reference/captures/login-session-2026-10-26.md`). This module answers that gate's question
//! from the rows, never from a hard-coded table.
//!
//! The pair must come from one **live** session: a closed session is a credential that was
//! surrendered, exactly as the auth service treats it
//! (`docs/research/gw2-service-design-lessons.md`, rule 1).

use crate::{PgPool, Result, Uuid};

/// The portal pair an email resolves to: the uid the gate hands out and the session it belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortalIdentity {
    /// The portal user id the client presents.
    pub portal_user: Uuid,
    /// The session row the pair was taken from.
    pub session: Uuid,
}

/// The live portal pair for an email, or `None` when that account has no open session.
///
/// `None` is what the web gate answers the way the reference does: HTTP 400, `who are you?`.
pub async fn for_email(pool: &PgPool, email: &str) -> Result<Option<PortalIdentity>> {
    let row = sqlx::query_as::<_, (Uuid, Uuid)>(
        "SELECT s.portal_user, s.id
           FROM t_session s
           JOIN t_account a ON a.id = s.account_id
          WHERE a.username = $1
            AND s.ended_at IS NULL
          ORDER BY s.started_at DESC
          LIMIT 1",
    )
    .bind(email.trim())
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(portal_user, session)| PortalIdentity {
        portal_user,
        session,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::Identity;

    /// A live database with a fresh account and session, or `None` to skip.
    ///
    /// The store does not own the pool or the connection settings yet - that is a later cut - so the
    /// test reads the same variable every program reads and skips when it is absent.
    async fn pool_with_identity(label: &str) -> Option<(PgPool, String, Uuid, Uuid)> {
        let url = std::env::var("ASURA_DATABASE_URL").ok()?;
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .ok()?;
        let email = format!("store-{label}-{}@example.test", std::process::id());
        let portal = Uuid::new_v4();
        let session = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO t_account (id, username) VALUES (gen_random_uuid(), $1)
             ON CONFLICT (username) DO NOTHING",
        )
        .bind(&email)
        .execute(&pool)
        .await
        .ok()?;
        sqlx::query(
            "INSERT INTO t_session (id, account_id, portal_user, started_at)
             SELECT $1, a.id, $2, now() FROM t_account a WHERE a.username = $3
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(session)
        .bind(portal)
        .bind(&email)
        .execute(&pool)
        .await
        .ok()?;
        Some((pool, email, portal, session))
    }

    async fn clean(pool: &PgPool, email: &str) {
        let _ = sqlx::query("DELETE FROM t_account WHERE username = $1")
            .bind(email)
            .execute(pool)
            .await;
    }

    #[tokio::test]
    async fn an_email_resolves_to_its_live_portal_pair() {
        let Some((pool, email, portal, session)) = pool_with_identity("live").await else {
            println!("ASURA_DATABASE_URL not set (or unreachable); skipping the portal store test");
            return;
        };

        let found = Identity::new(&pool)
            .portal_for_email(&email)
            .await
            .expect("lookup");
        assert_eq!(
            found,
            Some(PortalIdentity {
                portal_user: portal,
                session
            }),
            "the row's own pair"
        );
        assert_eq!(
            Identity::new(&pool)
                .portal_for_email("nobody-here@example.test")
                .await
                .expect("lookup"),
            None,
            "an unknown email has no identity"
        );

        clean(&pool, &email).await;
    }

    #[tokio::test]
    async fn a_closed_session_does_not_resolve() {
        let Some((pool, email, _portal, session)) = pool_with_identity("closed").await else {
            println!("ASURA_DATABASE_URL not set (or unreachable); skipping the portal store test");
            return;
        };
        sqlx::query("UPDATE t_session SET ended_at = now() WHERE id = $1")
            .bind(session)
            .execute(&pool)
            .await
            .expect("close the session");

        assert_eq!(
            Identity::new(&pool)
                .portal_for_email(&email)
                .await
                .expect("lookup"),
            None,
            "a surrendered session is not a credential"
        );

        clean(&pool, &email).await;
    }
}

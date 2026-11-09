//! M3 acceptance tests, moved with the fact they test: the identity pair resolves, and nothing else.
//!
//! These run against the live PostgreSQL named by `ASURA_DATABASE_URL`. They are harness-only: no
//! game client, no protocol. What they prove is the decision the auth service makes, taken here where
//! a failure is a test failure instead of an opaque `007` in front of a player.
//!
//! Two rules, from `docs/reference/client-protocol.md` section 4 and
//! `docs/research/gw2-service-design-lessons.md` rule 1:
//!
//! 1. the pair is validated on **both** values - a right portal user with a wrong session id is not
//!    close enough;
//! 2. a closed session never resolves, or a stolen session id would work forever.
//!
//! These came from `crates/asura-db/tests/login.rs` in cut 2b; the character tests stayed behind,
//! because a character is the state store's fact and moves in cut 2c.

use asura_store::identity::Identity;
use asura_store::{PgPool, Uuid};

async fn pool() -> Option<PgPool> {
    let url = std::env::var("ASURA_DATABASE_URL").ok()?;
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .ok()
}

fn skip() {
    println!("ASURA_DATABASE_URL not set; skipping live-database identity test");
}

fn account_name(label: &str) -> String {
    format!("m3-{label}-{}", Uuid::new_v4())
}

/// An account with one live session, seeded the way the store's own tests seed one.
async fn seed(pool: &PgPool, portal_user: Uuid, session: Uuid, name: &str) -> Uuid {
    let account: Uuid = sqlx::query_scalar(
        "INSERT INTO t_account (id, username) VALUES (gen_random_uuid(), $1)
         ON CONFLICT (username) DO UPDATE SET username = EXCLUDED.username
         RETURNING id",
    )
    .bind(name)
    .fetch_one(pool)
    .await
    .expect("seed the account");

    sqlx::query(
        "INSERT INTO t_session (id, account_id, portal_user, started_at, ended_at)
         VALUES ($1, $2, $3, now(), NULL)
         ON CONFLICT (id) DO UPDATE SET
             account_id  = EXCLUDED.account_id,
             portal_user = EXCLUDED.portal_user,
             started_at  = now(),
             ended_at    = NULL",
    )
    .bind(session)
    .bind(account)
    .bind(portal_user)
    .execute(pool)
    .await
    .expect("seed the session");

    account
}

async fn clean(pool: &PgPool, name: &str) {
    let _ = sqlx::query("DELETE FROM t_account WHERE username = $1")
        .bind(name)
        .execute(pool)
        .await;
}

#[tokio::test]
async fn a_presented_pair_resolves_to_its_account() {
    let Some(pool) = pool().await else {
        return skip();
    };
    let name = account_name("valid");
    let portal_user = Uuid::new_v4();
    let session = Uuid::new_v4();
    let account = seed(&pool, portal_user, session, &name).await;

    let resolved = Identity::new(&pool)
        .session(portal_user, session)
        .await
        .expect("query")
        .expect("the pair must resolve");

    assert_eq!(resolved.session, session);
    assert_eq!(resolved.account, account);
    assert_eq!(
        resolved.character, None,
        "nothing has been selected on a fresh session"
    );
    assert_ne!(
        account, portal_user,
        "the account is its own identity, not the portal user id"
    );

    clean(&pool, &name).await;
}

#[tokio::test]
async fn neither_half_of_the_pair_is_optional() {
    let Some(pool) = pool().await else {
        return skip();
    };
    let name = account_name("halves");
    let portal_user = Uuid::new_v4();
    let session = Uuid::new_v4();
    seed(&pool, portal_user, session, &name).await;

    let identity = Identity::new(&pool);

    // Right portal user, wrong session id: what a stale or forged client presents.
    assert_eq!(
        identity
            .session(portal_user, Uuid::new_v4())
            .await
            .expect("query"),
        None,
        "a wrong session id must not resolve"
    );
    // Right session id, wrong portal user: the same, from the other side.
    assert_eq!(
        identity
            .session(Uuid::new_v4(), session)
            .await
            .expect("query"),
        None,
        "a wrong portal user must not resolve"
    );
    // Both wrong: the same answer, not a different one.
    assert_eq!(
        identity
            .session(Uuid::new_v4(), Uuid::new_v4())
            .await
            .expect("query"),
        None
    );

    clean(&pool, &name).await;
}

#[tokio::test]
async fn a_closed_session_never_resolves_again() {
    let Some(pool) = pool().await else {
        return skip();
    };
    let name = account_name("closed");
    let portal_user = Uuid::new_v4();
    let session = Uuid::new_v4();
    seed(&pool, portal_user, session, &name).await;

    let identity = Identity::new(&pool);
    assert!(identity.close_session(session).await.expect("close"));

    assert_eq!(
        identity.session(portal_user, session).await.expect("query"),
        None,
        "a closed session must not resolve: the pair is spent, not a permanent key"
    );

    // Closing twice is a no-op rather than an error, because a disconnect and a stale sweep can both
    // reach the same session.
    assert!(!identity.close_session(session).await.expect("close again"));

    // Re-seeding the same pair reopens it, which is how a developer resets a spent session.
    seed(&pool, portal_user, session, &name).await;
    assert!(
        identity
            .session(portal_user, session)
            .await
            .expect("query")
            .is_some(),
        "re-seeding is the documented way back to a working pair"
    );

    clean(&pool, &name).await;
}

#[tokio::test]
async fn the_newest_session_wins_and_the_older_one_is_closed() {
    let Some(pool) = pool().await else {
        return skip();
    };
    let name = account_name("newest");

    // Two live sessions on one account, as a second client would open.
    let (older_portal, older_session) = (Uuid::new_v4(), Uuid::new_v4());
    let (newer_portal, newer_session) = (Uuid::new_v4(), Uuid::new_v4());
    let older_account = seed(&pool, older_portal, older_session, &name).await;
    let newer_account = seed(&pool, newer_portal, newer_session, &name).await;
    assert_eq!(older_account, newer_account, "one account, two sessions");

    let identity = Identity::new(&pool);
    assert!(
        identity
            .session(older_portal, older_session)
            .await
            .expect("query")
            .is_some(),
        "both sessions resolve before the rule is applied"
    );

    let closed = identity
        .close_other_sessions(newer_account, newer_session)
        .await
        .expect("close the others");
    assert_eq!(closed, 1, "exactly the older session is closed");

    assert!(
        identity
            .session(newer_portal, newer_session)
            .await
            .expect("query")
            .is_some(),
        "the newest session resolves"
    );
    assert_eq!(
        identity
            .session(older_portal, older_session)
            .await
            .expect("query"),
        None,
        "the older session does not"
    );

    clean(&pool, &name).await;
}

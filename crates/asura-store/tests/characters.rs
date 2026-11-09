//! M3 acceptance tests: the identity pair resolves, and nothing else does.
//!
//! These run against the live PostgreSQL named by `ASURA_DATABASE_URL` (the environment, or the
//! repository's `.env`). They are harness-only: no game client, no protocol. What they prove is the
//! decision the auth service will make, taken here where a failure is a test failure instead of an
//! opaque `007` in front of a player.
//!
//! With no database configured every test prints a skip line and returns.
//!
//! The rules under test, from `docs/reference/client-protocol.md` section 4 and `docs/research/gw2-service-design-lessons.md` rule 1:
//!
//! 1. the pair is validated on **both** values -- a right portal user with a wrong session id is not
//!    close enough;
//! 2. a closed session never resolves, or a stolen session id would work forever;
//! 3. a character list is scoped to its account, in the query, not after it;
//! 4. the newest session wins and the older one is closed.
//!
//! Every account name is unique per run (it carries a fresh UUID), so a run that panics before its
//! cleanup leaves an inert row that no later run can mistake for its own.

use asura_store::state::characters as db;
use asura_store::{PgPool, Uuid};

/// What the session names, read here because the identity store owns that row now (cut 2b). A test
/// may ask a table directly; the store is the only *program* path allowed to.
async fn session_character(pool: &PgPool, session: Uuid) -> Option<Option<Uuid>> {
    sqlx::query_scalar::<_, Option<Uuid>>(
        "SELECT char_id FROM t_session WHERE id = $1 AND ended_at IS NULL",
    )
    .bind(session)
    .fetch_optional(pool)
    .await
    .expect("the session row")
}

async fn close_session(pool: &PgPool, session: Uuid) {
    sqlx::query("UPDATE t_session SET ended_at = now() WHERE id = $1")
        .bind(session)
        .execute(pool)
        .await
        .expect("close the session");
}

fn account_name(label: &str) -> String {
    format!("m3-{label}-{}", Uuid::new_v4())
}
const MAP: i32 = 15;

async fn pool() -> Option<PgPool> {
    // One rule for finding the settings, in `asura_db::config`, shared with the services.
    let url = asura_db::config::database_url()?;
    Some(
        asura_db::placement::connect(&url, 4)
            .await
            .expect("connect pool"),
    )
}

fn skip() {
    println!("ASURA_DATABASE_URL not set; skipping live-database login test");
}

#[tokio::test]
async fn a_character_list_is_scoped_to_its_own_account() {
    let Some(pool) = pool().await else {
        return skip();
    };
    let mine = account_name("chars-mine");
    let theirs = account_name("chars-theirs");

    // Two characters on my account, seeded under one name so they share the account row.
    let seeded = db::seed_identity(
        &pool,
        Uuid::new_v4(),
        Uuid::new_v4(),
        &mine,
        "Asura One",
        MAP,
    )
    .await
    .expect("seed the first character");
    db::seed_identity(
        &pool,
        Uuid::new_v4(),
        Uuid::new_v4(),
        &mine,
        "Asura Two",
        MAP,
    )
    .await
    .expect("seed the second character");

    // Another account, whose characters must never appear in my list.
    let other = db::seed_identity(
        &pool,
        Uuid::new_v4(),
        Uuid::new_v4(),
        &theirs,
        "Someone Else",
        MAP,
    )
    .await
    .expect("seed the other account");

    let characters = db::characters(&pool, seeded.account)
        .await
        .expect("characters");
    let names: Vec<&str> = characters.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["Asura One", "Asura Two"], "ordered by name");
    assert!(
        characters.iter().all(|c| c.id != other.character),
        "another account's character must not leak into the list"
    );
    assert!(characters.iter().all(|c| c.map_id == MAP));

    let theirs_list = db::characters(&pool, other.account)
        .await
        .expect("characters");
    assert_eq!(theirs_list.len(), 1);
    assert_eq!(theirs_list[0].name, "Someone Else");

    db::remove_account(&pool, &mine).await.expect("cleanup");
    db::remove_account(&pool, &theirs).await.expect("cleanup");
}

#[tokio::test]
async fn selecting_a_character_records_it_on_the_live_session_only() {
    let Some(pool) = pool().await else {
        return skip();
    };
    let name = account_name("select");
    let seeded = db::seed_identity(
        &pool,
        Uuid::new_v4(),
        Uuid::new_v4(),
        &name,
        "Asura Test",
        MAP,
    )
    .await
    .expect("seed identity");

    assert!(
        db::select_character(&pool, seeded.session, seeded.character)
            .await
            .expect("select")
    );

    assert_eq!(
        session_character(&pool, seeded.session)
            .await
            .expect("the live session"),
        Some(seeded.character)
    );

    // A closed session is not a place to record a selection.
    close_session(&pool, seeded.session).await;
    assert!(
        !db::select_character(&pool, seeded.session, seeded.character)
            .await
            .expect("select on a closed session")
    );

    db::remove_account(&pool, &name).await.expect("cleanup");
}

#[tokio::test]
async fn deleting_a_character_stops_the_session_naming_it() {
    let Some(pool) = pool().await else {
        return skip();
    };
    let name = account_name("delete");
    let seeded = db::seed_identity(
        &pool,
        Uuid::new_v4(),
        Uuid::new_v4(),
        &name,
        "Asura Test",
        MAP,
    )
    .await
    .expect("seed identity");

    assert!(
        db::select_character(&pool, seeded.session, seeded.character)
            .await
            .expect("select")
    );
    assert_eq!(
        session_character(&pool, seeded.session)
            .await
            .expect("the live session"),
        Some(seeded.character)
    );

    assert!(
        db::delete_character(&pool, seeded.account, seeded.character)
            .await
            .expect("delete")
    );

    // The row is gone and the session no longer names it: a travel for this session cannot go out
    // for a character that does not exist.
    assert_eq!(
        session_character(&pool, seeded.session)
            .await
            .expect("the live session"),
        None,
        "the deleted character is not named by the session any more"
    );

    db::remove_account(&pool, &name).await.expect("cleanup");
}

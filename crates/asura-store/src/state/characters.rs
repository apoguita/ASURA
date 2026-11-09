//! The state store's character rows: the list an account may see, the one it selects, the one it deletes.
//!
//! Moved from the old crate's login module in cut 2c with the SQL unchanged. The identity half of that module - the
//! account row and the session pair - already left for `identity` in cut 2b; what is left here is the
//! character's own table.
//!
//! Two rules the SQL carries, from `docs/reference/client-protocol.md` section 4:
//!
//! * a character list is scoped to its account **in the query**, because a missing `WHERE` there is the
//!   one place a leak would be silent;
//! * deleting a character clears the session that names it in the same transaction, or every travel goes
//!   out for an id that no longer exists.
//!
//! `seed_identity` and `remove_account` are development and test support, not a service path.
use crate::{PgPool, Uuid};

/// One character, as the character-list message needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Character {
    /// The row`s own identifier.
    pub id: Uuid,
    /// The name, as the UTF-16 code units the wire carries.
    pub name: String,
    /// Where the character stands. The login batch reports it; the auth service does not move anyone.
    pub map_id: i32,
}

/// The characters this account may see, ordered by name so the list a client renders is stable.
///
/// Scoped by `account_id` in the query rather than filtered afterwards: a character list is the one
/// place where a missing `WHERE` would leak another player's data, so the scope belongs in the SQL.
pub async fn characters(pool: &PgPool, account: Uuid) -> sqlx::Result<Vec<Character>> {
    sqlx::query_as::<_, (Uuid, String, i32)>(
        "SELECT id, name, map_id
           FROM t_characters
          WHERE account_id = $1
          ORDER BY name",
    )
    .bind(account)
    .fetch_all(pool)
    .await
    .map(|rows| {
        rows.into_iter()
            .map(|(id, name, map_id)| Character { id, name, map_id })
            .collect()
    })
}

/// Record that a session chose this character. `char_id` is a routing hint for a reconnect
/// (`docs/guidance/architecture.md`, "Identity"), not an authorisation: the pair was already validated.
pub async fn select_character(pool: &PgPool, session: Uuid, character: Uuid) -> sqlx::Result<bool> {
    let done = sqlx::query(
        "UPDATE t_session SET char_id = $2
          WHERE id = $1 AND ended_at IS NULL",
    )
    .bind(session)
    .bind(character)
    .execute(pool)
    .await?;
    Ok(done.rows_affected() > 0)
}

/// Delete one character, scoped to the account that owns it.
///
/// The `account_id` in the `WHERE` is what makes the reference's rule
/// (`AuthSrv_HandleDeleteCharacter` matches the name against *that account's* characters) hold at the
/// storage layer too: a wrong account id deletes nothing rather than someone else's character.
///
/// The session that named the character stops naming it, in the same transaction. A deleted character
/// left behind in `session.char_id` makes every travel go out for an id that no longer exists, and the
/// world answers "the character named by the transfer is gone" - measured 2026-10-28, after five
/// characters were deleted by hand.
pub async fn delete_character(pool: &PgPool, account: Uuid, character: Uuid) -> sqlx::Result<bool> {
    let mut tx = pool.begin().await?;

    sqlx::query("UPDATE t_session SET char_id = NULL WHERE char_id = $1")
        .bind(character)
        .execute(&mut *tx)
        .await?;

    let done = sqlx::query("DELETE FROM t_characters WHERE account_id = $1 AND id = $2")
        .bind(account)
        .bind(character)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(done.rows_affected() > 0)
}

/// Everything a development login needs to exist: an account, a portal identity pair, and one
/// character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeededIdentity {
    /// The web gate`s user id, which a login is resolved from.
    pub portal_user: Uuid,
    /// The session id the gate issued for it.
    pub session: Uuid,
    /// The account the character belongs to.
    pub account: Uuid,
    /// The character the row is about.
    pub character: Uuid,
}

/// Create (or refresh) a development identity: `portal_user` + `session` -> account -> character.
///
/// **Development and test support, not a service path.** Two reasons it lives here rather than in a
/// script full of SQL: this crate is the only place that writes SQL, and the tests need exactly this
/// to build the state they prove. It is idempotent, so re-running it reopens the same session instead
/// of stacking rows - which is also how a developer resets a session that a test closed.
///
/// The account gets its own generated id rather than reusing the portal user id: they are different
/// things (GWPS keeps them in different columns), and a seed that conflates them would hide exactly
/// the bug this schema change exists to prevent.
pub async fn seed_identity(
    pool: &PgPool,
    portal_user: Uuid,
    session: Uuid,
    account_name: &str,
    character_name: &str,
    map_id: i32,
) -> sqlx::Result<SeededIdentity> {
    let mut tx = pool.begin().await?;

    // Upsert on the unique username and return the row's id either way. `DO UPDATE` is what makes
    // RETURNING fire for an existing row; `SET username = EXCLUDED.username` is a no-op write.
    let account: Uuid = sqlx::query_scalar(
        "INSERT INTO t_account (id, username) VALUES (gen_random_uuid(), $1)
         ON CONFLICT (username) DO UPDATE SET username = EXCLUDED.username
         RETURNING id",
    )
    .bind(account_name)
    .fetch_one(&mut *tx)
    .await?;

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
    .execute(&mut *tx)
    .await?;

    let character: Uuid = match sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM t_characters WHERE account_id = $1 AND name = $2",
    )
    .bind(account)
    .bind(character_name)
    .fetch_optional(&mut *tx)
    .await?
    {
        Some(existing) => existing,
        None => {
            sqlx::query_scalar(
                // `last_outpost` is written too: this fixture means "a character standing in this map",
                // and the placement rule asks where it was last placed before it lets it leave an
                // explorable. A row with the memory left at 0 reads as "never placed" and is sent to
                // the campaign default instead (D28, `asura_rules::placement`).
                "INSERT INTO t_characters (id, account_id, name, map_id, last_outpost)
             VALUES (gen_random_uuid(), $1, $2, $3, $3)
             RETURNING id",
            )
            .bind(account)
            .bind(character_name)
            .bind(map_id)
            .fetch_one(&mut *tx)
            .await?
        }
    };

    tx.commit().await?;

    Ok(SeededIdentity {
        portal_user,
        session,
        account,
        character,
    })
}

/// Remove what [`seed_identity`] created - an account and, by cascade, its sessions and characters.
///
/// Test hygiene: a run leaves nothing behind that the next run would mistake for a live account.
pub async fn remove_account(pool: &PgPool, account_name: &str) -> sqlx::Result<bool> {
    let done = sqlx::query("DELETE FROM t_account WHERE username = $1")
        .bind(account_name)
        .execute(pool)
        .await?;
    Ok(done.rows_affected() > 0)
}

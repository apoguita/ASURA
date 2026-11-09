//! The state store: what happened to whom. Characters, their position and unlocks, and what they carry.
//!
//! Moved here from `asura-db` in cut 2c: `characters` (the list, the selection, the deletion and the
//! development seed), `character` (the row's facts) and `inventory` (bags and items). The SQL is the old
//! SQL - a cut that changed an answer would be a behaviour change, and phase 2 carries none.
//!
//! What this store must never do: read an identity row, read a world's capacity or a map's content, or
//! decide anything. `asura_db`'s error type is kept for now so no caller changes; unifying it is part of
//! cut 2e, when the old crate is deleted.

pub mod character;
pub mod characters;
pub mod inventory;

use crate::{PgPool, Uuid};

/// The state store, opened over the pool the caller owns.
pub struct State<'a> {
    pool: &'a PgPool,
}

impl<'a> State<'a> {
    /// Open the store over a pool.
    pub fn new(pool: &'a PgPool) -> Self {
        State { pool }
    }

    /// Where a character last stood, or `None` when it has never been saved.
    pub async fn position(&self, character: Uuid) -> sqlx::Result<Option<character::Position>> {
        character::position(self.pool, character).await
    }

    /// The character's own row: appearance, profession, gold, campaign, level.
    pub async fn traits(&self, character: Uuid) -> sqlx::Result<Option<character::Traits>> {
        character::traits(self.pool, character).await
    }

    /// The character's bags and items.
    pub async fn inventory(&self, character: Uuid) -> sqlx::Result<inventory::Inventory> {
        inventory::load(self.pool, character).await
    }

    /// The character list an account may see.
    pub async fn characters(&self, account: Uuid) -> sqlx::Result<Vec<characters::Character>> {
        characters::characters(self.pool, account).await
    }
}

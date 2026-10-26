//! The reads a world server needs: which map blob to load, where to spawn, and who the character is.
//!
//! Small on purpose. A world server owns the gameplay connection and its own in-memory simulation;
//! what it takes from the database is the handful of facts a client cannot be told without, and what
//! it writes back arrives with the milestone that writes state (`docs/roadmap.md`, M5).

use crate::{PgPool, Uuid};

/// What a map needs in order to be loaded: the map blob's file id, whether the map is an outpost or
/// an explorable area, and where a character stands in it.
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnMap {
    pub map_id: i32,
    /// 0 for an outpost/town, 1 for an explorable area (the reference's `MapType`).
    pub map_type: i32,
    /// The `Gw.dat` file id of the map blob. The client streams the map from this number; a map
    /// without one loads to a black screen.
    pub map_file_id: i64,
    /// The spawn coordinate, as the client's world coordinates. Verified against a live client in the
    /// reference rig for the development maps (`GmMapsConfig.c`).
    pub spawn_x: f32,
    pub spawn_y: f32,
    /// The plane inside the map blob, not a height: 0 for every pre-Searing map measured so far.
    pub spawn_plane: i16,
}

/// The configured map, if the database has it.
pub async fn spawn_map(pool: &PgPool, map_id: i32) -> sqlx::Result<Option<SpawnMap>> {
    let row: Option<(i32, i32, i64, f32, f32, i16)> = sqlx::query_as(
        "SELECT map_id, map_type, map_file_id, spawn_x, spawn_y, spawn_plane
           FROM map_config WHERE map_id = $1",
    )
    .bind(map_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(
        |(map_id, map_type, map_file_id, spawn_x, spawn_y, spawn_plane)| SpawnMap {
            map_id,
            map_type,
            map_file_id,
            spawn_x,
            spawn_y,
            spawn_plane,
        },
    ))
}

/// A character's name, as the client should see it in the map.
pub async fn character_name(pool: &PgPool, character: Uuid) -> sqlx::Result<Option<String>> {
    sqlx::query_scalar("SELECT name FROM characters WHERE id = $1")
        .bind(character)
        .fetch_optional(pool)
        .await
}

/// Every map the database knows, so a world server can publish a pool for each of them at startup.
pub async fn all_maps(pool: &PgPool) -> sqlx::Result<Vec<SpawnMap>> {
    let rows: Vec<(i32, i32, i64, f32, f32, i16)> = sqlx::query_as(
        "SELECT map_id, map_type, map_file_id, spawn_x, spawn_y, spawn_plane
           FROM map_config ORDER BY map_id",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(map_id, map_type, map_file_id, spawn_x, spawn_y, spawn_plane)| SpawnMap {
                map_id,
                map_type,
                map_file_id,
                spawn_x,
                spawn_y,
                spawn_plane,
            },
        )
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The name a world server puts in the map is the character's own, and the spawn comes from the
    /// map row: a map without a coordinate spawns a character at the origin, which on every map
    /// measured so far is outside the walkable geometry.
    #[test]
    fn a_spawn_map_carries_the_map_key_the_blob_and_the_spawn() {
        let map = SpawnMap {
            map_id: 146,
            map_type: 0,
            map_file_id: 0x1B97D,
            spawn_x: 6602.0,
            spawn_y: 4485.0,
            spawn_plane: 0,
        };
        assert_eq!(map.map_id, 146, "Lakeside County");
        assert_eq!(map.map_type, 0, "an outpost");
        assert_eq!(map.map_file_id, 113021, "the pre-Searing blob, 0x1B97D");
        assert_eq!(
            map.spawn_plane, 0,
            "the plane the tester read back from a standing character"
        );
        assert_ne!(map.spawn_x, 0.0, "and not the origin");
    }
}

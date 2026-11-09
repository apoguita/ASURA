//! The content rows the old crate still answers for: the map blob, its spawn points and its doorways.
//!
//! These are `t_map_config`, `t_map_spawn` and `t_map_portal` - content that is becoming a pack file
//! (phase 6). Their readers here have no caller, which is why they were left behind when the
//! character state moved to `asura_store::state::character` in cut 2c.
//!
//! Two of those facts are a character's position, and this is where it is read and written: the world
//! saves it when a player leaves and starts from it when the player comes back. **The reference does not
//! do this** - it re-spawns every character at its map's spawn point on every session, and keeps only
//! the map a character was last in (`characters.last_outpost`). ASURA keeps the coordinate because the
//! whole of placement exists to answer "who is where", and a player who logs back in where it logged out
//! is the smallest thing that answer is worth. The seed leaves the column NULL, so a character that has
//! never been played still spawns where the map says.

use crate::PgPool;

/// What a map needs in order to be loaded: the map blob's file id, whether the map is an outpost or
/// an explorable area, and where a character stands in it.
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnMap {
    /// The map the row is about.
    pub map_id: i32,
    /// 0 for an outpost/town, 1 for an explorable area (the reference's `MapType`).
    pub map_type: i32,
    /// The `Gw.dat` file id of the map blob. The client streams the map from this number; a map
    /// without one loads to a black screen.
    pub map_file_id: i64,
    /// The spawn coordinate, as the client's world coordinates. Verified against a live client in the
    /// reference rig for the development maps (`GmMapsConfig.c`).
    pub spawn_x: f32,
    /// The spawn coordinate the map row carries.
    pub spawn_y: f32,
    /// The plane inside the map blob, not a height: 0 for every pre-Searing map measured so far.
    pub spawn_plane: i16,
    /// What to call it, so `/maps` and the log can name a map instead of printing its number. Empty when
    /// the database has no name for it - a worse answer, not a broken one.
    pub name: String,
}

/// A map row as [`SpawnMap`] reads it. One column list, used by both queries.
type MapRow = (i32, i32, i64, f32, f32, i16, String);

fn spawn_map_from_row(row: MapRow) -> SpawnMap {
    let (map_id, map_type, map_file_id, spawn_x, spawn_y, spawn_plane, name) = row;
    SpawnMap {
        map_id,
        map_type,
        map_file_id,
        spawn_x,
        spawn_y,
        spawn_plane,
        name,
    }
}

/// The configured map, if the database has it.
pub async fn spawn_map(pool: &PgPool, map_id: i32) -> sqlx::Result<Option<SpawnMap>> {
    let row: Option<MapRow> = sqlx::query_as(
        "SELECT map_id, map_type, map_file_id, spawn_x, spawn_y, spawn_plane, name
           FROM t_map_config WHERE map_id = $1",
    )
    .bind(map_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(spawn_map_from_row))
}

/// Every spawn point a map holds, in the reference's order (`GmMapsConfig.h`: `spawnpoints[]`), index 0
/// first. The world spawns a character at index 0, which is what `GameSrv.c:1075` does; the rest are the
/// capture's, kept because a map entered from a different portal has more than one.
pub async fn spawn_points(pool: &PgPool, map_id: i32) -> sqlx::Result<Vec<(f32, f32, i16)>> {
    sqlx::query_as::<_, (f32, f32, i16)>(
        "SELECT x, y, plane FROM t_map_spawn WHERE map_id = $1 ORDER BY idx",
    )
    .bind(map_id)
    .fetch_all(pool)
    .await
}

/// One doorway of `map_portal`: the instance it stands in, where, and - when known - the instance it
/// leads to and where a crossing character lands there.
pub type PortalRow = (i32, f32, f32, Option<i32>, Option<f32>, Option<f32>);

/// Every doorway the content defines (`0025_map_portals.sql`), map by map.
///
/// An `Err` here usually means the table does not exist yet (the schema has not been re-applied); the
/// world then keeps its compiled doorway table, which is the behaviour it had before the table existed.
pub async fn map_portals(pool: &PgPool) -> sqlx::Result<Vec<PortalRow>> {
    sqlx::query_as::<_, PortalRow>(
        "SELECT map_id, x, y, to_map, arrive_x, arrive_y FROM t_map_portal ORDER BY map_id, idx",
    )
    .fetch_all(pool)
    .await
}

/// Every map the catalogue describes, in id order.
///
/// What a world hosts when it is not told otherwise: the reference pre-spawns one server per configured
/// map, so a freshly started world here serves the same set. `--maps` narrows it on purpose (a soak, a
/// single-map test), never by default - a world hosting one map answers `/tp` with "not hosted" for the
/// rest, and `/maps` prints one alias.
pub async fn configured_map_ids(pool: &PgPool) -> sqlx::Result<Vec<i32>> {
    sqlx::query_scalar("SELECT map_id FROM t_map_config ORDER BY map_id")
        .fetch_all(pool)
        .await
}

/// Every map the database knows, so a world server can publish a pool for each of them at startup.
pub async fn all_maps(pool: &PgPool) -> sqlx::Result<Vec<SpawnMap>> {
    let rows: Vec<MapRow> = sqlx::query_as(
        "SELECT map_id, map_type, map_file_id, spawn_x, spawn_y, spawn_plane, name
           FROM t_map_config ORDER BY map_id",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(spawn_map_from_row).collect())
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
            name: "Lakeside County".to_string(),
        };
        assert_eq!(map.map_id, 146, "Lakeside County");
        assert_eq!(map.name, "Lakeside County", "and it says so");
        assert_eq!(map.map_type, 0, "an outpost");
        assert_eq!(map.map_file_id, 113021, "the pre-Searing blob, 0x1B97D");
        assert_eq!(
            map.spawn_plane, 0,
            "the plane the tester read back from a standing character"
        );
        assert_ne!(map.spawn_x, 0.0, "and not the origin");
    }
}

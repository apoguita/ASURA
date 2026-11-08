//! The map catalogue: one entry per map, and the questions a server answers about it.
//!
//! This is what a world publishes and what `/maps` lists. The entry is a row of the catalogue - a name,
//! a `Gw.dat` blob, a spawn point - and [`Map`] is the reference's own API over it, kept with the same
//! method names so a reader coming from the client's code finds them.
//!
//! Entries come from basic.json and spawns.json in the selected world pack.

/// The map API: the questions a server answers about one map. A member takes the catalogue entry
/// instead of reading a live client: the map's state here is a `MapEntry` row from `map_config`.
#[allow(non_snake_case)]
pub struct Map;

#[allow(non_snake_case)]
impl Map {
    /// `Map.GetMapID()`.
    pub fn GetMapID(map: &MapEntry) -> i32 {
        map.map_id
    }

    /// `Map.GetMapName()`.
    pub fn GetMapName(map: &MapEntry) -> &str {
        &map.name
    }

    /// `Map.GetInstanceType()`: `MapType_*`, 0 for an outpost and 1 for an explorable.
    pub fn GetInstanceType(map: &MapEntry) -> i32 {
        map.map_type
    }

    /// `Map.IsOutpost()`.
    pub fn IsOutpost(map: &MapEntry) -> bool {
        map.map_type == map_kinds().main_town
    }

    /// `Map.IsExplorable()`.
    pub fn IsExplorable(map: &MapEntry) -> bool {
        map.map_type == map_kinds().main_explorable
    }
}

/// The client's map kinds, as `config/Server/defaults.json` states them.
///
/// The world compares a catalogue row's `map_type` against these, so they arrive from the same file the
/// login path reads rather than from a constant (`docs/reference/decisions/layer-decisions.md`, D34). A world whose file
/// cannot be read stops at start-up, which is what makes the `expect` here unreachable rather than a
/// hazard.
pub fn map_kinds() -> asura_protocol::game::MapKinds {
    asura_config::defaults()
        .expect("the world refuses to start without config/Server/defaults.json")
        .map_kinds()
}

/// Build a hosted map entirely from the selected pack. Missing blob IDs are not filled from SQL.
pub fn entry(map_id: i32, capacity: i32) -> Result<Option<MapEntry>, String> {
    let maps = asura_config::world::catalogue()?;
    let Some(basic) = maps.get(&map_id) else { return Ok(None); };
    let map_file_id = basic.map_file_id.unwrap_or(0);
    if map_file_id == 0 && basic.map_type != map_kinds().character_creation { return Ok(None); }
    let spawns = crate::map_spawns::for_map(map_id).unwrap_or_default().to_vec();
    let spawn = spawns.first().map(|spawn| (spawn.x, spawn.y)).unwrap_or_default();
    let spawn_plane = spawns.first().map(|spawn| spawn.plane).unwrap_or_default();
    Ok(Some(MapEntry {
        map_id, map_file_id, map_type: basic.map_type, spawn, spawn_plane, spawns,
        name: basic.name.clone(), capacity,
    }))
}

#[cfg(test)]
mod file_tests {
    #[test]
    fn northlands_uses_the_live_file_id_from_the_pack() {
        // Live capture northlands-live-20261106-014922.jsonl, record 511, StoC 0x0196.
        assert_eq!(super::entry(147, 1).unwrap().unwrap().map_file_id, 116025);
    }
}

/// One map a world server publishes, with what the client needs to load it.
#[derive(Debug, Clone, PartialEq)]
pub struct MapEntry {
    pub map_id: i32,
    /// The `Gw.dat` file id of the map blob (`map_config.map_file_id`). A map without one loads to a
    /// black screen, so it is passed to the client rather than guessed.
    pub map_file_id: u32,
    /// 0 for an outpost, 1 for an explorable area.
    pub map_type: i32,
    /// The single coordinate the catalogue row carries, used when the map's own spawn list is empty.
    /// The reference's own coordinate, read back from a live client in the rig; (0, 0) is outside
    /// the walkable geometry on every map measured so far.
    pub spawn: (f32, f32),
    /// The plane inside the map blob.
    pub spawn_plane: u16,
    /// The map's spawn points, as the pack states them (`spawns.json`, `docs/reference/decisions/layer-decisions.md`
    /// D39). An outpost is entered at one of these, picked at random; an empty list means the pack
    /// states none, and [`crate::Standing::from_map`] answers instead.
    pub spawns: Vec<crate::map_spawns::MapSpawn>,
    /// What the map is called, for `/maps` and the log. Empty is allowed: a map without a name prints
    /// its id, which is a worse answer than the reference's and not a broken one.
    pub name: String,
    /// How many players this world will place in it. `/maps` reports it because a player asking "where
    /// can I go" wants to know what is running, not only what exists.
    pub capacity: i32,
}

impl MapEntry {
    /// How the map is named in an answer or a log line: its name when it has one, its id when it does
    /// not, and never an empty string.
    pub fn label(&self) -> String {
        if self.name.is_empty() {
            format!("map {}", self.map_id)
        } else {
            format!("{} ({})", self.name, self.map_id)
        }
    }

    /// The alias `/tp` would take for this map: the reference's aliases are lower-case names with single
    /// spaces ("lakeside county"), so this is the name lower-cased. An unnamed map has no alias to type and
    /// says so, rather than inventing one.
    pub fn alias(&self) -> String {
        let alias = self.name.to_lowercase();
        let collapsed = alias.split_whitespace().collect::<Vec<_>>().join(" ");
        if collapsed.is_empty() {
            format!("map {}", self.map_id)
        } else {
            collapsed
        }
    }

    /// `0` and `1` are the reference's own `MapType` values; every other value it can carry is a kind of
    /// instance this world does not host.
    pub fn kind(&self) -> &'static str {
        match self.map_type {
            0 => "outpost",
            1 => "explorable",
            _ => "other",
        }
    }
}

//! The map catalogue: one entry per map, and the questions a server answers about it.
//!
//! This is what a world publishes and what `/maps` lists. The entry is a row of the catalogue - a name,
//! a `Gw.dat` blob, a spawn point - and [`Map`] is the reference's own API over it, kept with the same
//! method names so a reader coming from the client's code finds them.
//!
//! The entries arrive from `t_map_config` today and from a world package later (`docs/38-layer-decisions.md`,
//! D20-D24). Keeping them here means that move never touches the load.

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
        map.map_type == MAP_TYPE_MAIN_TOWN
    }

    /// `Map.IsExplorable()`.
    pub fn IsExplorable(map: &MapEntry) -> bool {
        map.map_type == MAP_TYPE_MAIN_EXPLORABLE
    }
}

/// The three map kinds, owned by `asura-rules` (`crates/asura-rules/src/lib.rs`): placement decides by
/// them before a world exists, so this crate reads them rather than keeping a second copy
/// (`docs/38-layer-decisions.md`, D27).
pub use asura_rules::{MAP_TYPE_CHARACTER_CREATION, MAP_TYPE_MAIN_EXPLORABLE, MAP_TYPE_MAIN_TOWN};

/// One map a world server publishes, with what the client needs to load it.
#[derive(Debug, Clone, PartialEq)]
pub struct MapEntry {
    pub map_id: i32,
    /// The `Gw.dat` file id of the map blob (`map_config.map_file_id`). A map without one loads to a
    /// black screen, so it is passed to the client rather than guessed.
    pub map_file_id: u32,
    /// 0 for an outpost, 1 for an explorable area.
    pub map_type: i32,
    /// Where a character stands on entering. The reference's own coordinates, read back from a live
    /// client in the rig; (0, 0) is outside the walkable geometry on every map measured so far.
    pub spawn: (f32, f32),
    /// The plane inside the map blob.
    pub spawn_plane: u16,
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

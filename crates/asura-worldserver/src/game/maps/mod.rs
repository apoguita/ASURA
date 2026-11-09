//! The maps: instances, the catalogue they come from, and the geometry that answers for them.
//!
//! The reference's `game/Maps` and `game/Grids`. This project has no grids - the client's own map
//! data is read from the archive (`pathfind`, `map_data`, `path_tables`) - and the parallel update
//! arrives with cut 4c.

pub mod cartography;
pub mod catalogue;
pub mod instance;
pub mod instance_table;
pub mod map;
pub mod map_data;
pub mod map_spawns;
pub mod path_tables;
pub mod pathfind;
pub mod portals;
pub mod spawns;

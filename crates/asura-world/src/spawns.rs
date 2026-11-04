//! Where a created character is spawned, by its expansion source.
//!
//! The character-creation screen asks the player which expansion their character belongs to, and the
//! confirm writes the map below into the character's row before the client reconnects into it. That map
//! is content - a measured value per source - so it lives in the world pack
//! (`config/World/Vanilla/character_spawn.json`, `docs/38-layer-decisions.md` D23, D24) rather than in a
//! `match` compiled into this crate.
//!
//! A source whose map has not been measured says so, and a source with no map at all (`pvp`) is `null`:
//! the creation refuses that character rather than putting it somewhere nobody has seen.

use std::path::{Path, PathBuf};

/// The pack a world reads when nothing says otherwise.
pub const DEFAULT_PACK: &str = "config/World/Vanilla";

/// The environment variable that names a different pack.
pub const PACK_VARIABLE: &str = "ASURA_WORLD_PACK";

/// The file inside a pack that holds the creation spawn points.
pub const SPAWNS_FILE: &str = "character_spawn.json";

/// One expansion source and where its characters are created.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct Spawn {
    /// The campaign number the creation request carries: 0 none (a PvP character), 1 Prophecies,
    /// 2 Factions, 3 Nightfall, 4 Eye of the North.
    pub campaign: u8,
    /// The expansion's name, for the log and for whoever reads the file.
    pub source: String,
    /// The map the character is written into, or `None` when no map has been measured.
    pub map_id: Option<i32>,
    /// Whether that map came from a recording. An unmeasured map is still used, and the log says so.
    #[serde(default)]
    pub measured: bool,
    /// What the map is, and where the number came from.
    #[serde(default)]
    pub doc: Option<String>,
}

/// The whole file: one spawn per expansion source, plus the fallback.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct CharacterSpawns {
    /// Free text for whoever opens the file.
    #[serde(default)]
    pub note: Option<String>,
    /// One entry per expansion source, in any order; the campaign number is the key.
    pub spawns: Vec<Spawn>,
    /// Where a character goes when its source is none of the above.
    pub fallback: Spawn,
}

/// Where a character of one expansion source is spawned.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    /// The map the character is written into.
    pub map_id: i32,
    /// The expansion's own name, for the log.
    pub source: String,
    /// Whether the map is a measurement, or the stand-in for a source nobody measured.
    pub measured: bool,
}

impl CharacterSpawns {
    /// Parse the file's text.
    pub fn from_json(text: &str) -> Result<CharacterSpawns, String> {
        serde_json::from_str(text).map_err(|err| err.to_string())
    }

    /// Where a character of this campaign number is spawned.
    ///
    /// `None` means the source is known and its map is not: the caller refuses the creation instead of
    /// choosing a map on the player's behalf. An unknown campaign gets the fallback, marked unmeasured so
    /// the log says a stand-in was used.
    pub fn for_campaign(&self, campaign: u8) -> Option<Placement> {
        if let Some(spawn) = self.spawns.iter().find(|spawn| spawn.campaign == campaign) {
            return spawn.map_id.map(|map_id| Placement {
                map_id,
                source: spawn.source.clone(),
                measured: spawn.measured,
            });
        }
        self.fallback.map_id.map(|map_id| Placement {
            map_id,
            source: self.fallback.source.clone(),
            measured: false,
        })
    }
}

/// The pack in force: `ASURA_WORLD_PACK`, or [`DEFAULT_PACK`].
pub fn pack() -> PathBuf {
    match std::env::var(PACK_VARIABLE) {
        Ok(value) if !value.trim().is_empty() => PathBuf::from(value.trim()),
        _ => PathBuf::from(DEFAULT_PACK),
    }
}

/// Read `<pack>/character_spawn.json`.
///
/// A missing file is an error here rather than a silent default: a world that created characters without
/// knowing where they spawn would put them somewhere nobody chose. The caller decides what to do, and
/// the creation path treats it as a refusal with the file's message.
pub fn load(pack: &Path) -> Result<CharacterSpawns, String> {
    let path = pack.join(SPAWNS_FILE);
    let text = std::fs::read_to_string(&path)
        .map_err(|err| format!("{} is not readable: {err}", path.display()))?;
    CharacterSpawns::from_json(&text)
        .map_err(|err| format!("{} is not a spawn file: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed pack, read the way the world reads it.
    const VANILLA: &str = include_str!("../../../config/World/Vanilla/character_spawn.json");

    fn vanilla() -> CharacterSpawns {
        CharacterSpawns::from_json(VANILLA).expect("the committed pack parses")
    }

    #[test]
    fn every_expansion_source_has_an_entry() {
        let spawns = vanilla();
        for campaign in [0u8, 1, 2, 3, 4] {
            assert!(
                spawns.spawns.iter().any(|spawn| spawn.campaign == campaign),
                "campaign {campaign} is missing"
            );
        }
    }

    #[test]
    fn the_two_measured_sources_name_their_maps() {
        let spawns = vanilla();
        let prophecies = spawns.for_campaign(1).expect("prophecies");
        assert_eq!(prophecies.map_id, 148);
        assert!(prophecies.measured);
        let factions = spawns.for_campaign(2).expect("factions");
        assert_eq!(factions.map_id, 212);
        assert!(factions.measured);
    }

    #[test]
    fn an_unmeasured_source_says_so_rather_than_looking_measured() {
        let spawns = vanilla();
        let nightfall = spawns.for_campaign(3).expect("nightfall");
        assert_eq!(nightfall.map_id, 449);
        assert!(
            !nightfall.measured,
            "not a measurement, and the log must say so"
        );
        assert!(!spawns.for_campaign(4).expect("eye of the north").measured);
    }

    #[test]
    fn a_pvp_character_is_refused_rather_than_guessed() {
        let spawns = vanilla();
        assert!(
            spawns.for_campaign(0).is_none(),
            "no measured map, so the creation must refuse"
        );
    }

    #[test]
    fn an_unknown_source_gets_the_fallback_and_is_not_called_measured() {
        let spawns = vanilla();
        let placement = spawns.for_campaign(42).expect("fallback");
        assert_eq!(placement.map_id, 148);
        assert!(
            !placement.measured,
            "the caller has to say a stand-in was used"
        );
    }
}

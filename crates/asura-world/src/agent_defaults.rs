//! What every player agent is created with: the speed it walks at, its health, its energy, and the
//! regeneration the load block states.
//!
//! They are content - values that describe the world - so they live in the world pack
//! (`config/World/Vanilla/agent_defaults.json`, `docs/reference/decisions/layer-decisions.md`, D23,
//! D24, D35) rather than as literals in the wire crate or in the loader. Two of them reach the client:
//! `speed` is the `speed_base` of `WORLD_CREATE_AGENT`, and health and energy go out as agent
//! int-properties. The simulation advances an agent by the same `speed`, which is why the value is
//! read once at start-up and copied into each instance rather than looked up per tick.

use std::path::Path;

use crate::character_defaults::Value;

/// The file inside a pack that holds the agent defaults.
pub const FILE: &str = "agent_defaults.json";

/// What every player agent is created with.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct AgentDefaults {
    /// Free text for whoever opens the file.
    #[serde(default)]
    pub note: Option<String>,
    /// Units a second the client walks a player agent at, and the simulation moves it by.
    pub speed: Value<f32>,
    /// The health a new player agent has.
    pub health: Value<u32>,
    /// The energy a new player agent has.
    pub energy: Value<u32>,
    /// The energy regeneration stated in the load block.
    pub energy_regen: Value<f32>,
}

impl AgentDefaults {
    /// Parse the file's text. Every key is required: a missing one is an error, not a default.
    pub fn from_json(text: &str) -> Result<AgentDefaults, String> {
        serde_json::from_str(text).map_err(|err| err.to_string())
    }

    /// The walking speed, as the number rather than the wrapper.
    pub fn speed(&self) -> f32 {
        self.speed.value
    }

    /// The health, as the number rather than the wrapper.
    pub fn health(&self) -> u32 {
        self.health.value
    }

    /// The energy, as the number rather than the wrapper.
    pub fn energy(&self) -> u32 {
        self.energy.value
    }

    /// The energy regeneration, as the number rather than the wrapper.
    pub fn energy_regen(&self) -> f32 {
        self.energy_regen.value
    }
}

/// Read `<pack>/agent_defaults.json`. A missing file is an error: the world refuses to start rather
/// than creating agents at a speed nobody chose.
pub fn load(pack: &Path) -> Result<AgentDefaults, String> {
    let path = pack.join(FILE);
    let text = std::fs::read_to_string(&path)
        .map_err(|err| format!("{} is not readable: {err}", path.display()))?;
    AgentDefaults::from_json(&text)
        .map_err(|err| format!("{} is not an agent defaults file: {err}", path.display()))
}

/// What a player agent is created with, read once per process.
///
/// The pack is configuration, so it is read at first use and held; start-up checks it so a bad file
/// fails there rather than on the first agent.
pub fn current() -> Result<&'static AgentDefaults, String> {
    static IN_FORCE: std::sync::OnceLock<Result<AgentDefaults, String>> =
        std::sync::OnceLock::new();
    IN_FORCE
        .get_or_init(|| {
            let path = crate::spawns::pack();
            load(&path).map(|defaults| {
                eprintln!("asura: agent defaults from {}", path.display());
                defaults
            })
        })
        .as_ref()
        .map_err(|err| err.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed pack, read the way the world reads it.
    const VANILLA: &str = include_str!("../../../config/World/Vanilla/agent_defaults.json");

    #[test]
    fn the_committed_file_states_the_reference_values() {
        let defaults = AgentDefaults::from_json(VANILLA).expect("the committed file parses");
        assert_eq!(defaults.speed(), 288.0, "the reference's walking speed");
        assert_eq!(defaults.health(), 400);
        assert_eq!(defaults.energy(), 25);
        assert_eq!(defaults.energy_regen(), 0.0);
    }

    #[test]
    fn every_key_documents_itself() {
        let defaults = AgentDefaults::from_json(VANILLA).expect("parses");
        for (name, doc) in [
            ("speed", &defaults.speed.doc),
            ("health", &defaults.health.doc),
            ("energy", &defaults.energy.doc),
            ("energy_regen", &defaults.energy_regen.doc),
        ] {
            assert!(
                doc.as_deref().is_some_and(|text| !text.is_empty()),
                "{name} says nothing"
            );
        }
    }

    #[test]
    fn a_file_without_a_key_is_an_error_not_a_default() {
        let err = AgentDefaults::from_json(r#"{"speed": {"value": 288.0}}"#).expect_err("refused");
        assert!(err.contains("health"), "said: {err}");
    }

    #[test]
    fn a_missing_file_is_an_error() {
        let err = load(Path::new("no/such/pack")).expect_err("refused");
        assert!(err.contains("agent_defaults.json"), "said: {err}");
    }
}

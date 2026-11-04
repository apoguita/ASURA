//! What a character is given when it is created.
//!
//! The confirm writes these onto the character's row beside the appearance word the client sent and the
//! profession the screen chose. They are content - values that describe what a new character is - so they
//! live in the world pack (`config/World/Vanilla/character_defaults.json`, `docs/38`, D23, D24, D35)
//! rather than as literals in the confirm.
//!
//! The unlocked-profession bitmap is deliberately not here: it is one bit for the profession the player
//! chose, which the confirm derives. A value is something the world states; a bitmap of the chosen
//! profession is a decision about it.

use std::path::Path;

/// The file inside a pack that holds the character defaults.
pub const FILE: &str = "character_defaults.json";

/// One documented value, the same shape every configuration file here uses.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct Value<T> {
    /// The value itself.
    pub value: T,
    /// What it means.
    #[serde(default)]
    pub doc: Option<String>,
}

/// What a new character is given.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct CharacterDefaults {
    /// Free text for whoever opens the file.
    #[serde(default)]
    pub note: Option<String>,
    /// The level a new character starts at.
    pub level: Value<u16>,
    /// The gold a new character starts with.
    pub gold: Value<u32>,
    /// The secondary profession of a new character.
    pub secondary_profession: Value<u8>,
}

impl CharacterDefaults {
    /// Parse the file's text. Every key is required: a missing one is an error, not a default.
    pub fn from_json(text: &str) -> Result<CharacterDefaults, String> {
        serde_json::from_str(text).map_err(|err| err.to_string())
    }

    /// The level, as the number rather than the wrapper.
    pub fn level(&self) -> u16 {
        self.level.value
    }

    /// The gold, as the number rather than the wrapper.
    pub fn gold(&self) -> u32 {
        self.gold.value
    }

    /// The secondary profession, as the number rather than the wrapper.
    pub fn secondary_profession(&self) -> u8 {
        self.secondary_profession.value
    }
}

/// Read `<pack>/character_defaults.json`. A missing file is an error: the confirm refuses the character
/// rather than inventing what a new one is worth.
pub fn load(pack: &Path) -> Result<CharacterDefaults, String> {
    let path = pack.join(FILE);
    let text = std::fs::read_to_string(&path)
        .map_err(|err| format!("{} is not readable: {err}", path.display()))?;
    CharacterDefaults::from_json(&text)
        .map_err(|err| format!("{} is not a character defaults file: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed pack, read the way the world reads it.
    const VANILLA: &str = include_str!("../../../config/World/Vanilla/character_defaults.json");

    #[test]
    fn the_committed_file_states_what_a_new_character_is() {
        let defaults = CharacterDefaults::from_json(VANILLA).expect("the committed file parses");
        assert_eq!(defaults.level(), 1);
        assert_eq!(defaults.gold(), 0);
        assert_eq!(defaults.secondary_profession(), 0);
        assert!(
            defaults.level.doc.is_some() && defaults.gold.doc.is_some(),
            "every value says what it is"
        );
    }

    #[test]
    fn every_key_documents_itself() {
        let defaults = CharacterDefaults::from_json(VANILLA).expect("parses");
        for (name, doc) in [
            ("level", &defaults.level.doc),
            ("gold", &defaults.gold.doc),
            ("secondary_profession", &defaults.secondary_profession.doc),
        ] {
            assert!(
                doc.as_deref().is_some_and(|text| !text.is_empty()),
                "{name} says nothing"
            );
        }
    }

    #[test]
    fn a_file_without_a_key_is_an_error_not_a_default() {
        let err = CharacterDefaults::from_json(r#"{"level": {"value": 1}}"#).expect_err("refused");
        assert!(err.contains("gold"), "said: {err}");
    }

    #[test]
    fn a_missing_file_is_an_error() {
        let err = load(Path::new("no/such/pack")).expect_err("refused");
        assert!(err.contains("character_defaults.json"), "said: {err}");
    }
}

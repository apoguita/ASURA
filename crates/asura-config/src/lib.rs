//! The intervals this machine runs at, and the file that holds them.
//!
//! These are the numbers a client's behaviour is measured against: how often the world ticks, how often
//! it asks whether the client is still there, how long login waits for a world's port, and how old a
//! world's heartbeat may be before it counts as gone. They were constants in three different crates
//! (`asura-world`'s `connection.rs`, `asura-auth`'s `travel.rs`, `asura-rules` for the freshness window),
//! so changing one meant editing Rust and rebuilding, and no single place answered "what cadence is this
//! server using" (`docs/38-layer-decisions.md`, D33).
//!
//! The file is `config/Server/intervals.json`. It is read once per process and then held, because every
//! value in it is a cadence: re-reading it mid-session would make a server run at two cadences.
//!
//! Precedence, and it is deliberate: an environment variable overrides the file, and the file overrides
//! the built-in default. The env variable is what a one-off experiment uses; the file is what a machine
//! runs on; the default is what happens when a checkout has no `config/` yet.

use std::sync::OnceLock;

/// The file a machine's intervals are read from.
pub const DEFAULT_FILE: &str = "config/Server/intervals.json";

/// The variable that names a different file, for an experiment.
pub const FILE_VARIABLE: &str = "ASURA_INTERVALS";

/// The variable that overrides `world_max_age_secs` for one run. Spelled here, next to the file that
/// carries the default, so both readers name it the same way.
pub const MAX_AGE_VARIABLE: &str = "ASURA_WORLD_MAX_AGE_SECS";

/// Every interval this server runs at.
///
/// The defaults are the numbers the three crates already used, so a checkout with no file behaves
/// exactly as it did before the file existed. `deny_unknown_fields` is on purpose: a misspelled key in a
/// hand-edited file would otherwise be silently ignored, and the server would run at the default while
/// its operator believed otherwise.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Intervals {
    /// How often the world wakes up: the tick that carries elapsed time to the client.
    ///
    /// Measured from the live client (`recordings/client/feature/movement/movement-sniff.log`): a
    /// `0x001E` every ~100 ms, median 81 ms. The reference's own floor is 500 ms.
    pub tick_interval_ms: u64,
    /// How often the world asks the client whether it is still there. The reference's constant is 5000 ms
    /// (`GAME_SRV_TIME_BETWEEN_PING_MS`).
    pub ping_interval_secs: u64,
    /// The value the world answers a ping with. **Not a measurement**: GWPS sends 20 and this server has
    /// never verified it against our client.
    pub ping_value: u32,
    /// How long login waits for a candidate world's port to answer before dropping it from the list.
    /// A loopback world answers in microseconds; a silent one should not cost the client the operating
    /// system's connect timeout.
    pub world_probe_timeout_ms: u64,
    /// Free text for whoever opens the file, including where each number came from. Never read by the
    /// server: JSON has no comments, so the file carries one string instead.
    #[serde(default)]
    pub note: Option<String>,
    /// How stale a world''s heartbeat may be and still be offered, routed to, and believed.
    ///
    /// Two programs read it - the login path and the socket layer - so it lives in one place; disagreeing
    /// means a client is sent to a world the next step refuses (`docs/38`, D29).
    pub world_max_age_secs: f64,
}

impl Default for Intervals {
    fn default() -> Self {
        Intervals {
            tick_interval_ms: 100,
            ping_interval_secs: 5,
            ping_value: 20,
            world_probe_timeout_ms: 500,
            note: None,
            world_max_age_secs: 60.0,
        }
    }
}

impl Intervals {
    /// The tick as a duration.
    pub fn tick(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.tick_interval_ms)
    }

    /// The ping cadence as a duration.
    pub fn ping(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.ping_interval_secs)
    }

    /// The probe timeout as a duration.
    pub fn probe_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.world_probe_timeout_ms)
    }

    /// Parse the file's text. Every key is optional: a file may state only what this machine changes.
    pub fn from_json(text: &str) -> Result<Intervals, String> {
        serde_json::from_str(text).map_err(|err| err.to_string())
    }
}

/// Where the values in force came from, so a start-up can say it once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Read from this file.
    File(String),
    /// No file: the built-in defaults are in force.
    Defaults,
}

/// The intervals in force, and where they came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    /// The values.
    pub intervals: Intervals,
    /// Where they came from.
    pub source: Source,
}

/// The workspace root, as this crate's own manifest directory sees it.
///
/// A service is started from the repository root and finds `config/` in its working directory; a test is
/// started from the crate's directory and does not. Both are the same file, so both are tried, working
/// directory first.
fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// Resolve a configured path: absolute stays as it is, and a relative one is tried against the working
/// directory and then against the workspace root.
fn resolve(path: &str) -> std::path::PathBuf {
    let candidate = std::path::Path::new(path);
    if candidate.is_absolute() || candidate.exists() {
        return candidate.to_path_buf();
    }
    let rooted = workspace_root().join(candidate);
    if rooted.exists() {
        return rooted;
    }
    candidate.to_path_buf()
}

/// The path in force: `ASURA_INTERVALS`, or [`DEFAULT_FILE`].
pub fn path() -> String {
    match std::env::var(FILE_VARIABLE) {
        Ok(value) if !value.trim().is_empty() => value.trim().to_string(),
        _ => DEFAULT_FILE.to_string(),
    }
}

/// Read the file. A missing file is the defaults, because a checkout without `config/` still runs; a file
/// that exists and does not parse is an error, because a server running at numbers nobody chose is worse
/// than a server that refuses to start and says why.
pub fn load() -> Result<Loaded, String> {
    let path = resolve(&path());
    let path = path.display().to_string();
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(Loaded {
            intervals: Intervals::from_json(&text)
                .map_err(|err| format!("{path} is not a valid intervals file: {err}"))?,
            source: Source::File(path),
        }),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Loaded {
            intervals: Intervals::default(),
            source: Source::Defaults,
        }),
        Err(err) => Err(format!("{path} is not readable: {err}")),
    }
}

/// The process's intervals, read once.
///
/// Every value is a cadence, so it is read at first use and then held: a file edited mid-session does not
/// make one connection tick at 100 ms and the next at 200.
pub fn intervals() -> &'static Intervals {
    static IN_FORCE: OnceLock<Intervals> = OnceLock::new();
    IN_FORCE.get_or_init(|| match load() {
        Ok(loaded) => {
            match &loaded.source {
                Source::File(path) => eprintln!("asura: intervals from {path}"),
                Source::Defaults => eprintln!(
                    "asura: no intervals file at {}; using the built-in defaults",
                    path()
                ),
            }
            loaded.intervals
        }
        Err(err) => {
            // A broken file is not worth crashing a running world over here, but it must be visible:
            // silently falling back would run the server at numbers nobody asked for.
            eprintln!("asura: {err}; using the built-in defaults");
            Intervals::default()
        }
    })
}

/// The file that holds the values a world falls back on when a row says nothing.
pub const DEFAULTS_FILE: &str = "config/Server/defaults.json";

/// The variable that names a different defaults file.
pub const DEFAULTS_VARIABLE: &str = "ASURA_DEFAULTS";

/// One documented value: the number, and the sentence that says what it means.
///
/// The documentation travels with the value because JSON has no comments and a file of bare numbers is a
/// file nobody can review. A key without `doc` is accepted - a name can be enough - but the base game's
/// own file documents every one.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct Described<T> {
    /// The value itself.
    pub value: T,
    /// What it means, in the words the person reading the file needs.
    #[serde(default)]
    pub doc: Option<String>,
    /// `true` only where changing the value would break what the client expects - the client's own
    /// numbers. Absent, or `false`, means the owner may change it (`docs/38-layer-decisions.md`, D35).
    #[serde(default)]
    pub immutable: bool,
}

/// The values a world falls back on when its rows say nothing.
///
/// Every key is required and there is no built-in value: a world that placed a character somewhere
/// nobody configured would be running on a number that does not exist (`docs/38-layer-decisions.md`,
/// D34).
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Defaults {
    /// Where a character with no placement history is put. Written by character creation, by a scripted
    /// event and by travelling for every other case; this is the fallback for a row where none of them
    /// happened.
    pub default_outpost: Described<i32>,
    /// The creation instance's map id: the destination that is a conversation rather than a place, and
    /// the value a row carries when nothing has been written yet.
    pub creation_map: Described<i32>,
    /// The client's `MapType` value for a town or outpost.
    pub map_type_main_town: Described<i32>,
    /// The client's `MapType` value for an explorable area.
    pub map_type_main_explorable: Described<i32>,
    /// The client's `MapType` value for the character-creation instance.
    pub map_type_character_creation: Described<i32>,
}

impl Defaults {
    /// Parse the file's text. A file without `default_outpost` is an error, not a default.
    pub fn from_json(text: &str) -> Result<Defaults, String> {
        serde_json::from_str(text).map_err(|err| err.to_string())
    }

    /// The fallback outpost, as the number rather than the wrapper.
    pub fn default_outpost(&self) -> i32 {
        self.default_outpost.value
    }

    /// The creation instance's map id, as the number rather than the wrapper.
    pub fn creation_map(&self) -> i32 {
        self.creation_map.value
    }

    /// The keys this file marks immutable, in a stable order, for the start-up line.
    ///
    /// Only the client's own numbers are here; every other value in the file is the owner's to change.
    pub fn immutable_keys(&self) -> Vec<&'static str> {
        let mut keys = Vec::new();
        if self.creation_map.immutable {
            keys.push("creation_map");
        }
        if self.map_type_main_town.immutable {
            keys.push("map_type_main_town");
        }
        if self.map_type_main_explorable.immutable {
            keys.push("map_type_main_explorable");
        }
        if self.map_type_character_creation.immutable {
            keys.push("map_type_character_creation");
        }
        keys
    }

    /// The three map kinds the placement rule asks about, as one value.
    pub fn map_kinds(&self) -> asura_protocol::game::MapKinds {
        asura_protocol::game::MapKinds {
            main_town: self.map_type_main_town.value,
            main_explorable: self.map_type_main_explorable.value,
            character_creation: self.map_type_character_creation.value,
        }
    }
}

/// The defaults in force, read once per process.
///
/// There is no built-in value: a missing file, an unreadable one, or one without `default_outpost` is an
/// error, and the service that needs it refuses to start.
pub fn defaults() -> Result<&'static Defaults, String> {
    static IN_FORCE: OnceLock<Result<Defaults, String>> = OnceLock::new();
    IN_FORCE
        .get_or_init(|| {
            let path = match std::env::var(DEFAULTS_VARIABLE) {
                Ok(value) if !value.trim().is_empty() => value.trim().to_string(),
                _ => DEFAULTS_FILE.to_string(),
            };
            let path = resolve(&path).display().to_string();
            match std::fs::read_to_string(&path) {
                Ok(text) => Defaults::from_json(&text)
                    .map_err(|err| format!("{path} is not a valid defaults file: {err}"))
                    .map(|defaults| {
                        eprintln!("asura: defaults from {path}");
                        let fixed = defaults.immutable_keys();
                        if !fixed.is_empty() {
                            eprintln!(
                                "asura: immutable (the client's contract, stated for review): {}",
                                fixed.join(", ")
                            );
                        }
                        defaults
                    }),
                Err(err) => Err(format!("{path} is not readable: {err}")),
            }
        })
        .as_ref()
        .map_err(|err| err.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed file, parsed the way a service parses it.
    const COMMITTED: &str = include_str!("../../../config/Server/intervals.json");

    /// The committed defaults file, parsed the way the services parse it.
    const DEFAULTS_COMMITTED: &str = include_str!("../../../config/Server/defaults.json");

    #[test]
    fn the_committed_file_states_the_defaults() {
        // The file and the defaults cannot drift: a change to either turns this red. The note is free
        // text, so only the numbers are compared.
        let from_file = Intervals::from_json(COMMITTED).expect("the committed file parses");
        let defaults = Intervals::default();
        assert_eq!(from_file.tick_interval_ms, defaults.tick_interval_ms);
        assert_eq!(from_file.ping_interval_secs, defaults.ping_interval_secs);
        assert_eq!(from_file.ping_value, defaults.ping_value);
        assert_eq!(
            from_file.world_probe_timeout_ms,
            defaults.world_probe_timeout_ms
        );
        assert_eq!(from_file.world_max_age_secs, defaults.world_max_age_secs);
        assert!(
            from_file.note.is_some(),
            "the file says where its numbers came from"
        );
    }

    #[test]
    fn the_committed_defaults_file_states_and_documents_the_outpost() {
        let defaults = Defaults::from_json(DEFAULTS_COMMITTED).expect("the committed file parses");
        assert_eq!(
            defaults.default_outpost(),
            148,
            "the base game's starting outpost"
        );
        let doc = defaults
            .default_outpost
            .doc
            .as_deref()
            .expect("the key documents itself");
        assert!(
            doc.contains("creation") && doc.contains("travelling"),
            "the doc says who writes the real value, said: {doc}"
        );
        assert_eq!(defaults.creation_map(), 0, "the creation instance is map 0");
        assert!(
            defaults
                .creation_map
                .doc
                .as_deref()
                .is_some_and(|doc| doc.contains("creation")),
            "the key documents itself"
        );
    }

    #[test]
    fn a_defaults_file_without_the_outpost_is_an_error_not_a_default() {
        let err = Defaults::from_json("{}").expect_err("refused");
        assert!(err.contains("default_outpost"), "said: {err}");
    }

    #[test]
    fn a_key_may_carry_its_own_documentation() {
        let defaults = Defaults::from_json(
            r#"{"default_outpost": {"value": 163, "doc": "why this number", "immutable": false},
                    "creation_map": {"value": 0, "immutable": true},
                    "map_type_main_town": {"value": 0, "immutable": true},
                    "map_type_main_explorable": {"value": 1, "immutable": true},
                    "map_type_character_creation": {"value": 11, "immutable": true}}"#,
        )
        .expect("parses");
        assert_eq!(defaults.default_outpost(), 163);
        assert_eq!(
            defaults.default_outpost.doc.as_deref(),
            Some("why this number")
        );
    }

    #[test]
    fn a_file_may_state_only_what_this_machine_changes() {
        let intervals = Intervals::from_json(r#"{"tick_interval_ms": 200}"#).expect("parses");
        assert_eq!(intervals.tick_interval_ms, 200);
        assert_eq!(intervals.ping_interval_secs, 5, "the rest are defaults");
        assert_eq!(intervals.ping_value, 20);
    }

    #[test]
    fn a_misspelled_key_is_an_error_rather_than_a_silent_default() {
        let err = Intervals::from_json(r#"{"tick_interval_msec": 200}"#).expect_err("refused");
        assert!(err.contains("tick_interval_msec") || err.contains("unknown field"));
    }

    #[test]
    fn the_durations_are_the_numbers_the_file_states() {
        let intervals = Intervals::from_json(
            r#"{"tick_interval_ms": 250, "ping_interval_secs": 3, "world_probe_timeout_ms": 750}"#,
        )
        .expect("parses");
        assert_eq!(intervals.tick(), std::time::Duration::from_millis(250));
        assert_eq!(intervals.ping(), std::time::Duration::from_secs(3));
        assert_eq!(
            intervals.probe_timeout(),
            std::time::Duration::from_millis(750)
        );
    }
}

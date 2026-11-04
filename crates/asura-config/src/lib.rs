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
    let path = path();
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed file, parsed the way a service parses it.
    const COMMITTED: &str = include_str!("../../../config/Server/intervals.json");

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

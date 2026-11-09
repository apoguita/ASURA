//! Chat: spoken lines, and the command table they arrive through.
//!
//! The reference has no chat handler; it prints and answers into its own log
//! (`docs/work/session-handoff.md`). This module is that half, plus the `!`/`#`/`$` channel rule and the
//! `/maps`, `/stuck` and `/tp` command table ([`Command`], [`command`]) a player types into a
//! chat line. The travel `/tp` starts lives in `travel.rs`; this module only decides which
//! command a line names.

use crate::*;

/// The reference's `/maps`, line for line (`GmCommand_ListTargets`).
///
/// It prints two things and they are **not the same text**, which is the part I had wrong until a client
/// tester pointed out that the list no longer showed every map:
///
/// * the **chat** side is one alias per map -- the first in the table, because that is the one to type --
///   comma-separated and wrapped inside one chat fragment, then the reference's fixed
///   "`Use: /tp <name>, e.g. /tp lakeside county`";
/// * the **log** side is **every alias**, one row each, with the map it names, in the reference's own column
///   widths and in its table order:
///
/// ```text
/// Teleport targets (20 aliases, every one with a map config):
///   /tp ascalon          -> map 148  Ascalon City (pre-Searing)
///   /tp ascalon city     -> map 148  Ascalon City (pre-Searing)
/// ```
///
/// The header's count is the **alias** count, not the map count, and the loop below prints a row per alias:
/// collapsing it to one row per map (which is what the chat half does) is how `/maps` stopped naming the
/// aliases a tester has to type.
///
/// The table is what a person reads, and it is the half that has always worked: no chat text has ever
/// reached a client's screen, in the reference or here, and GWPS's own chat gate leaves the chat half in its
/// log with the rest. So this returns the log lines, in that format, and `/maps` prints them. The chat
/// fragments are built too, because they are the answer's other half and a caller may one day have a channel
/// that renders them -- but `/maps` does not send them: sending chat crashed a live client here.
///
/// Only the aliases of maps **this world hosts** are listed, so a narrowed `--maps` does not advertise a
/// target `/tp` would refuse. With the default (the whole catalogue) that is the reference's whole table.
/// A map the reference's table does not name gets one row of its own, named by its id, rather than vanishing.
pub fn maps_answer(maps: &[MapEntry]) -> MappedMaps {
    let hosted: Vec<i32> = maps.iter().map(|map| map.map_id).collect();

    // The rows, in the reference's table order: one per alias.
    let mut rows: Vec<(String, i32, String)> = Vec::new();
    let mut chat: Vec<String> = Vec::new();
    let mut seen: Vec<i32> = Vec::new();
    for target in travel_targets::TRAVEL_TARGETS {
        if !hosted.contains(&target.map_id) {
            continue;
        }
        rows.push((
            target.alias.to_string(),
            target.map_id,
            target.label.to_string(),
        ));
        if !seen.contains(&target.map_id) {
            seen.push(target.map_id);
            chat.push(target.alias.to_string());
        }
    }
    for map in maps {
        // A creation instance is not a place: it is where a character is made, it has no alias, and
        // nobody travels to it. It stays out of the teleport table.
        if map.map_type == crate::map_kinds().character_creation {
            continue;
        }
        if seen.contains(&map.map_id) {
            continue;
        }
        seen.push(map.map_id);
        let alias = map.alias();
        chat.push(alias.clone());
        rows.push((alias, map.map_id, map.name.clone()));
    }

    // The chat line, split the way the reference splits it: `100 <= used + needed` starts a continuation
    // line indented by six spaces, each entry followed by a comma.
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::from("Maps:");
    for alias in &chat {
        if 100 <= current.len() + alias.len() + 3 {
            lines.push(current);
            current = String::from("      ");
        }
        current.push_str(&format!(" {alias},"));
    }
    lines.push(current);
    // The reference's own sentence, literal: it names `lakeside county` whatever is in the table.
    lines.push("Use: /tp <name>, e.g. /tp lakeside county".to_string());

    // The table the log gets: the alias count in the header, then every alias.
    let mut table = vec![format!(
        "Teleport targets ({} aliases, every one with a map config):",
        rows.len()
    )];
    for (alias, map_id, label) in &rows {
        table.push(format!("  /tp {alias:<15} -> map {map_id:>3}  {label}"));
    }

    MappedMaps { chat: lines, table }
}

/// What `/maps` produces: the lines the reference would put on the chat channel, and the table it puts in
/// its log. Two shapes, because the reference has two.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappedMaps {
    /// The channel messages this answer carries.
    pub chat: Vec<String>,
    /// The table rows this answer prints.
    pub table: Vec<String>,
}

/// `UPDATE_AGENT_VISUAL_EQUIPMENT`: what the agent is wearing, read out of the equipped-items bag.
/// How a map alias is compared: lower case, runs of spaces collapsed. The reference's own rule for
/// `/tp` (`GmCommand_ToAscii`).
pub fn alias_key(text: &str) -> String {
    text.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// One chat command this world answers.
///
/// The table is the single place the command surface is written down, and the handler dispatches through
/// it, so `tools/parity-commands.py` comparing it with the reference cannot drift from what the code does.
/// The reference's own surface is two functions: `GameSrv_TryHandleGmCommand` (`GmCommands.c`, which owns
/// `/tp`, `/teleport` and `/maps`) and `GameSrv_HandleChatCommand` (`GmChat.c`, which owns `/stuck`).
#[derive(Debug, Clone, Copy)]
pub struct Command {
    /// What a player types, without the slash. `GmCommands.c` accepts two names for one command.
    pub names: &'static [&'static str],
    /// Where the reference's own dispatch handles it, for a reader who wants to check.
    pub reference: &'static str,
    /// Whether the command takes an argument (`GameSrv_TryHandleGmCommand` splits on the first space).
    pub takes_argument: bool,
    /// What answering it does.
    pub answer: &'static str,
}

/// Every chat command this world answers, in the reference's order.
pub const COMMANDS: &[Command] = &[
    Command {
        names: &["tp", "teleport"],
        reference: "GmCommands.c:184 GameSrv_TryHandleGmCommand -> GameSrv_HandleTeleportCommand",
        takes_argument: true,
        answer: "a server transfer, or one of the reference's three refusals (unknown map, no config, already here)",
    },
    Command {
        names: &["maps"],
        reference: "GmCommands.c:206 GameSrv_TryHandleGmCommand -> GmCommand_ListTargets",
        takes_argument: false,
        answer: "the alias table, printed: the reference's chat half is gated off, so nothing goes on the wire",
    },
    Command {
        names: &["stuck"],
        reference: "GmChat.c:243 GameSrv_HandleChatCommand",
        takes_argument: false,
        answer: "AGENT_UPDATE_POSITION for the agent -- a position, not AGENT_STOP_MOVING",
    },
];

/// The command a player typed, if this world answers it.
pub fn command(name: &str) -> Option<&'static Command> {
    COMMANDS
        .iter()
        .find(|candidate| candidate.names.contains(&name))
}

/// The reference's `/tp` lookup: normalise the argument, find it in its table.
///
/// `GmCommand_ToAscii` refuses anything that is not printable ASCII, which is why a non-ASCII argument is
/// an unknown command rather than a fuzzy match; and the table is exact, so `lakesidecounty` is not
/// `lakeside county` even though the reference's own comment says otherwise.
pub fn travel_target(argument: &str) -> Option<&'static travel_targets::TravelTarget> {
    let wanted = alias_key(argument);
    if wanted.is_empty() {
        return None;
    }
    travel_targets::TRAVEL_TARGETS
        .iter()
        .find(|target| target.alias == wanted)
}

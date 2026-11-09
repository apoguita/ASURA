//! The handlers: one file per noun, each turning client messages into a noun's answers.
//!
//! The reference keeps this directory (`src/server/game/Handlers/`) with one handler per noun -
//! `MovementHandler`, `ItemHandler`, `ChatHandler`, `CharacterHandler` and thirty more - and its game
//! library holds the rules. This is the same arrangement: a handler reads a frame, asks a noun, and
//! answers; the noun owns its state and its rules. Cut 4d moved the item handlers here. The agent handlers are not in this directory: in this tree they
// hold the movement rules themselves, so moving them would separate a file from its name and not a
// handler from a rule. They leave when the rules do (the agent split, 4d's second half).

pub mod agent;
pub mod chat;
pub mod creation;
pub mod item;
pub mod travel;

use crate::*;

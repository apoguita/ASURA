//! The entities: the character as a player, the body as an agent, and the items in its bags.
//!
//! The reference's `game/Entities` (`Object` -> `Unit` -> `Player`/`Creature`, plus `Item`,
//! `GameObject` and the rest). Each is one noun with its own file, which is the whole reason this
//! directory exists.

pub mod agent;
pub mod item;
pub mod player;

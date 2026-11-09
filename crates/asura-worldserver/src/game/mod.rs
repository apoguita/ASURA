//! The game library: one directory per game noun, and the socket layer that serves them.
//!
//! The reference keeps `src/server/game` as a static library of noun subsystems - `Entities`, `Maps`,
//! `Grids`, `Spells`, `Skills`, `Groups`, `Quests`, `Loot`, `Chat`, `Movement`, `Server` and the rest -
//! with its own socket layer inside it (`game/Server/WorldSocket`, `WorldSession`). This crate copies
//! that arrangement; the directories arrive with the cuts that fill them
//! (`docs/work/re-engineering-plan.md`, phase 4).
//!
//! The nouns this project needs, and the cut each arrives with:
//!
//! * `server` - sockets and sessions (cut 4b). The reference's name for it.
//! * `entities` - player, agent, item (cut 4c).
//! * `maps` - instances, the spawn catalogue, the clock, the parallel update (cut 4c).
//! * `movement`, `items`, `travel`, `chat`, `creation` - mechanics (cut 4d).
//! * `scripting` - the hook position behaviour reaches mechanics through (cut 4e).

// Nothing yet: cut 4a builds the program's shell and its socket half (`shared`), and the first noun
// arrives with cut 4b. An empty module with a stated plan is better than a placeholder that pretends.

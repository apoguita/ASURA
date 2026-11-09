//! Behaviour against hooks: the engine publishes, content answers.
//!
//! The reference puts this at `game/Scripting/ScriptMgr` - 345 `On...` methods (`OnMapUpdate`,
//! `OnCreatureUpdate`, `OnPlayerLogin`, `OnSpellCheckCast`, ...) that content in `scripts/` and in
//! `modules/` implements without the engine ever naming a quest, a dungeon or a boss. This module is
//! that position for this project; the hooks arrive with cut 4e, when the mechanics can be reached
//! from behaviour instead of from a dispatcher list.

/// The hook registry.
///
/// Empty on purpose: a hook that no mechanic can yet answer would be a name with nothing behind it. It
/// exists now so the program's shape says where behaviour will be reached from, which is the one thing
/// that is expensive to move later.
#[derive(Debug, Default)]
pub struct Hooks;

impl Hooks {
    /// A registry with nothing registered.
    pub fn new() -> Self {
        Hooks
    }

    /// How many hooks are registered.
    pub fn len(&self) -> usize {
        0
    }

    /// Whether no hook is registered. A registry with none is the honest state of cut 4a.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_registry_is_empty() {
        assert_eq!(Hooks::new().len(), 0, "no mechanic is reachable yet");
    }
}

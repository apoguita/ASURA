//! Game rules, as decisions the server makes before a world exists.
//!
//! This crate exists because the auth service was answering a game question
//! (`docs/38-layer-decisions.md`, D25). Login has to say *where* a character enters the game, but
//! "where does a character start" is a rule of the game, not of the protocol, so it lives here and
//! both services can call it.
//!
//! Every function here is pure: the caller reads the rows, hands them in, and gets a decision back.
//! Nothing in this crate opens a connection, reads a file or prints. That is what keeps it callable
//! from the auth service, from the world, and from a test with no database.
//!
//! The rules are the owner's, recorded in `docs/22-world-model.md` ("Spawn points, and the outpost
//! rule") and `docs/38-layer-decisions.md` (D18, D19).

/// What the client asked for: a map id and the map type it named with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Requested {
    /// The instance the client named.
    pub map_id: i32,
    /// The map type it named alongside it: 0 outpost, 1 explorable, 11 creation instance.
    pub map_type: u32,
}

/// What the character's rows say about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Character {
    /// The character's map from its own row (`characters.map_id`): where it was written.
    pub map_id: i32,
    /// The last **outpost** it was placed in (`characters.last_outpost`), 0 when it never was.
    pub last_outpost: i32,
}

/// The creation instance's map id. The client calls it type 11 and asks for map 0.
pub const CREATION_MAP: i32 = 0;
/// `MapType_CharacterCreation`.
pub const MAP_TYPE_CHARACTER_CREATION: u32 = 11;
/// `MapType_MainTown`: the outpost kind, which the outpost rule never overrides.
pub const MAP_TYPE_MAIN_TOWN: i32 = 0;
/// Where a character with no history enters: Ascalon City, the stand-in outpost for a campaign whose
/// own starting outpost this tree has not measured (`docs/22-world-model.md`).
pub const DEFAULT_OUTPOST: i32 = 148;

/// Why a placement answer differs from what the client asked for. The caller logs it; the rule does
/// not print.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Substituted {
    /// The client named the creation instance as a place; the character's own map is the answer.
    CreationNamedAsPlace(&'static str),
    /// The destination is not an outpost, and the outpost rule put the character in its last one.
    NotAnOutpost,
}

/// The map a character is placed in when it enters the game.
///
/// Three inputs decide it, and the order matters:
///
/// 1. A creation visit (`map_type` 11, map 0) is never substituted: the client is opening the creation
///    screen, not entering the world.
/// 2. A client that names the creation instance as a place (`map_type` 0, map 0) is asking the
///    mistaken question that follows a creation confirm. The answer is the character's own row, then
///    its last outpost, then [`DEFAULT_OUTPOST`] - never map 0, which has no map file and leaves the
///    client on a loading screen.
/// 3. Any other destination that is not a main town is replaced by the last outpost, because entering
///    the game puts the character in the last outpost it visited.
///
/// `destination_type` is the map type of the map being asked for, as the caller read it, or `None`
/// when the map is not in the catalogue - an unknown map is left alone for the caller to refuse.
pub fn placement(
    request: Requested,
    character: Character,
    destination_type: Option<i32>,
) -> (i32, Option<Substituted>) {
    // 1. A creation visit is its own destination.
    if request.map_id == CREATION_MAP && request.map_type == MAP_TYPE_CHARACTER_CREATION {
        return (request.map_id, None);
    }

    // 2. The mistaken creation request: map 0 named as a place.
    if request.map_id == CREATION_MAP {
        if character.map_id != CREATION_MAP {
            return (
                character.map_id,
                Some(Substituted::CreationNamedAsPlace("the character's row")),
            );
        }
        if character.last_outpost != CREATION_MAP {
            return (
                character.last_outpost,
                Some(Substituted::CreationNamedAsPlace("its last outpost")),
            );
        }
        return (
            DEFAULT_OUTPOST,
            Some(Substituted::CreationNamedAsPlace("the default outpost")),
        );
    }

    // 3. The outpost rule, for every destination that is not a main town.
    let Some(map_type) = destination_type else {
        return (request.map_id, None);
    };
    if map_type == MAP_TYPE_MAIN_TOWN {
        return (request.map_id, None);
    }
    let outpost = if character.last_outpost != CREATION_MAP {
        character.last_outpost
    } else {
        DEFAULT_OUTPOST
    };
    if outpost == request.map_id {
        (request.map_id, None)
    } else {
        (outpost, Some(Substituted::NotAnOutpost))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRESH: Character = Character {
        map_id: CREATION_MAP,
        last_outpost: CREATION_MAP,
    };

    #[test]
    fn a_creation_visit_reaches_the_creation_instance() {
        let request = Requested {
            map_id: CREATION_MAP,
            map_type: MAP_TYPE_CHARACTER_CREATION,
        };
        assert_eq!(placement(request, FRESH, Some(11)), (CREATION_MAP, None));
    }

    #[test]
    fn the_mistaken_creation_request_goes_to_the_characters_row() {
        // What the client sends after a creation confirm.
        let request = Requested {
            map_id: CREATION_MAP,
            map_type: MAP_TYPE_MAIN_TOWN as u32,
        };
        let character = Character {
            map_id: 212,
            last_outpost: CREATION_MAP,
        };
        let (map, why) = placement(request, character, Some(11));
        assert_eq!(map, 212, "the map the confirm wrote");
        assert_eq!(
            why,
            Some(Substituted::CreationNamedAsPlace("the character's row"))
        );
    }

    #[test]
    fn a_character_with_no_row_map_falls_back_to_its_last_outpost() {
        let request = Requested {
            map_id: CREATION_MAP,
            map_type: MAP_TYPE_MAIN_TOWN as u32,
        };
        let character = Character {
            map_id: CREATION_MAP,
            last_outpost: 163,
        };
        assert_eq!(placement(request, character, Some(11)).0, 163);
    }

    #[test]
    fn a_character_with_no_history_gets_the_default_outpost() {
        let request = Requested {
            map_id: CREATION_MAP,
            map_type: MAP_TYPE_MAIN_TOWN as u32,
        };
        assert_eq!(placement(request, FRESH, Some(11)).0, DEFAULT_OUTPOST);
    }

    #[test]
    fn an_outpost_request_is_answered_as_asked() {
        let request = Requested {
            map_id: 163,
            map_type: MAP_TYPE_MAIN_TOWN as u32,
        };
        let character = Character {
            map_id: 163,
            last_outpost: 163,
        };
        assert_eq!(placement(request, character, Some(0)), (163, None));
    }

    #[test]
    fn an_explorable_request_goes_to_the_last_outpost() {
        let request = Requested {
            map_id: 146,
            map_type: 1,
        };
        let character = Character {
            map_id: 146,
            last_outpost: 148,
        };
        assert_eq!(
            placement(request, character, Some(1)),
            (148, Some(Substituted::NotAnOutpost))
        );
    }

    #[test]
    fn a_character_that_never_saw_an_outpost_and_asks_for_an_explorable_gets_the_default() {
        let request = Requested {
            map_id: 146,
            map_type: 1,
        };
        assert_eq!(
            placement(request, FRESH, Some(1)),
            (DEFAULT_OUTPOST, Some(Substituted::NotAnOutpost))
        );
    }

    #[test]
    fn an_unknown_map_is_left_for_the_caller_to_refuse() {
        let request = Requested {
            map_id: 999,
            map_type: 1,
        };
        assert_eq!(placement(request, FRESH, None), (999, None));
    }
}

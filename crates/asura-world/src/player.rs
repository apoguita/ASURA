//! The player: the character one connection drives, and the block that describes it.
//!
//! The server's `Player` - the counterpart of Py4GW_Stealth's `player.py`. [`player_block`] is
//! the character's own `GAME_SMSG_INSTANCE_PLAYER_DATA_*` run; the constants below are the
//! reference's defaults for it (`PLAYER_ID`, `AGENT_ID`, `PARTY_ID`) and the profession it
//! reports when a character row carries none.

use crate::*;

/// The player: the character one connection drives, and the instance it is in.
///
/// The server's `Player` - Py4GW_Stealth's `player.py`, same nomenclature - as a record: the
/// state this server owns, with the domain's verbs on it (`player.GetAgentID()`).
///
/// Assembled after the token pair is believed and before the load block, so that everything the blocks
/// need - whose character this is, where it stands, what it carries - is decided in one place rather than
/// read again in the middle of a conversation.
pub struct Player {
    /// The character the transfer named. Every write on the way out is scoped to it.
    pub character: Uuid,
    /// This connection's player id, allocated when it joined. The reference allocates the same number
    /// from `srv->players` (`GmIdAllocate`) and puts it in `GAME_SMSG_PLAYER_CREATE`, every agent's
    /// `model_id`, the party messages and the chat speaker field.
    pub player_id: u32,
    /// The instance this player is in. A map is an instance: this row is its description
    /// (map id, type, spawn, capacity) until the `Instance` object exists.
    pub instance: MapEntry,
    pub name: String,
    pub inventory: Inventory,
    pub agents: Agents,
    /// The login session and account the transfer named.
    ///
    /// The reference keeps both on its player (`GmPlayer.account_id`, `.char_id`) and uses them for every
    /// bag and item query and for its transfer request (`GameSrv.c:1450-1472`). This server scoped its
    /// writes by character alone, which is enough for everything it did until a *world* had to start a
    /// travel: the transfer row is keyed by session and needs the account as well (`docs/09` #9).
    pub session_id: Uuid,
    pub account_id: Uuid,
    /// Set when this connection has been redirected to another map. Its teardown must then leave the
    /// lease alone: the seat it holds now belongs to the connection the client is about to open.
    pub moved: bool,
    /// What a character-creation connection has chosen so far: `Some` when this connection is in a
    /// creation instance (map type 11), which is the reference's `MapType_CharacterCreation`.
    pub creation: Option<Creation>,
    /// The character's stored traits - what the load block sends instead of constants.
    pub traits: db_game::Traits,
    /// When the connection began, for the load time the reference reports.
    pub started: Instant,
    /// Movement counters, for the trace's closing summary.
    pub move_stats: MoveStats,
}

#[allow(non_snake_case)]
impl Player {
    /// Where the agent is **now**, which is what the reference answers the spawn and players requests
    /// with (`agent->position`): after the client has walked, that is no longer where it entered.
    pub fn standing(&self) -> Standing {
        Standing {
            position: self.agents.player.position,
            plane: self.agents.player.plane,
        }
    }
    /// `Player.GetAgentID()`: the agent this character drives.
    pub fn GetAgentID(&self) -> u32 {
        self.agents.player.agent_id
    }

    /// `Player.GetPlayerNumber()`: the player id the wire calls the player.
    pub fn GetPlayerNumber(&self) -> u32 {
        self.player_id
    }

    /// `Player.GetName()`: what `GAME_SMSG_PLAYER_CREATE` calls the character.
    pub fn GetName(&self) -> &str {
        &self.name
    }

    /// `Player.GetXY()`: where the character stands, as the server last believed it.
    pub fn GetXY(&self) -> (f32, f32) {
        self.agents.player.position
    }

    /// `Player.GetAgent()`: the agent record itself.
    pub fn GetAgent(&self) -> &AgentState {
        &self.agents.player
    }

    /// `Player.GetPlayerUUID()`: the character's uuid, the one the login connection carried.
    pub fn GetPlayerUUID(&self) -> Uuid {
        self.character
    }

    /// `Player.GetGoldOnCharacter()`: the gold the load block reports.
    pub fn GetGoldOnCharacter(&self) -> u32 {
        self.traits.gold
    }
}

/// The one player and one agent this world server has.
///
/// The reference allocates both from per-server id tables (thousands of agents, hundreds of players).
/// ASURA has one character in a map, so the id table is a constant -'- and the party message carries
/// these as `u16`, which is the only real constraint. It becomes a real allocator when there is more
/// than one player to allocate for.
pub const PLAYER_ID: u32 = 1;

/// See [`PLAYER_ID`].
pub const AGENT_ID: u32 = 1;

/// The party a lone player is put in.
pub const PARTY_ID: u16 = 1;

/// `GameSrv_CreatePlayerAgent`'s arbitrary team token; the client keeps it for the session and the
/// reference never gives it a meaning.
pub const PLAYER_TEAM_TOKEN: u32 = 0xBAAD_F00D;

/// The primary profession the reference reports for its default character, read off its send log
/// (`GAME_SMSG_PLAYER_UPDATE_PROFESSION`: `01`). 0 is `Profession_None`, which is not a character a
/// client can render.
pub const DEFAULT_PRIMARY_PROFESSION: u8 = 1;

/// The unlocked-profession bitmap the reference sends (`GAME_SMSG_PLAYER_UPDATE_UNLOCKED_PROFESSIONS`:
/// `02 00 00 00`), i.e. the bit for the profession above.
pub const DEFAULT_UNLOCKED_PROFESSIONS: u32 = 2;

/// Everything the client is told about the players in the instance, in the reference's order
/// (`GameSrv_HandleInstanceLoadRequestPlayers`).
///
/// Two parameters are there for the two things this block no longer takes from a constant: `standing`,
/// which places the agent where the character actually is (`WORLD_CREATE_AGENT`), and `inventory`, which
/// says what the agent is wearing (`UPDATE_AGENT_VISUAL_EQUIPMENT`) - the nine ids read out of the
/// equipped-items bag, because they belong to *this* character's items.
pub fn player_block(
    name: &str,
    standing: Standing,
    inventory: &Inventory,
    started: Instant,
    player_id: u32,
    agent_id: u32,
    traits: db_game::Traits,
) -> Vec<Frame> {
    let mut frames = Vec::with_capacity(30);

    // The PvP unlocks. The reference sends its own fixed table of 387 unlockable items
    // (`GameSrv_SendPvpItems`, generated into `game::pvp_items`), and the client builds its
    // unlockable-item table from them, so they are copied rather than skipped.
    frames.push(game::pvp_update_unlocked_skills(&[]));
    frames.push(game::pvp_update_unlocked_heroes(&[0xFFFF_FFFF; 8]));
    for item in game::pvp_items::PVP_ITEMS {
        frames.push(game::pvp_item_stream_add(item.id, item.params));
    }
    frames.push(game::pvp_item_stream_end());

    // The account features, exactly the reference's list (`GameSrv_SendAccountFeatures`).
    for (feature, param1, param2) in [
        (1u16, 10u16, 0u16),
        (99, 3, 0),
        (100, 5, 0),
        (101, 5, 5),
        (102, 2, 2),
        (111, 1, 0),
        (124, 1, 0),
        (125, 1, 0),
        (131, 1, 0),
    ] {
        frames.push(game::account_feature(feature, param1, param2));
    }

    // The unlocked-map list. The three bitmaps a character can have progress in are sent at their
    // **full length, zero-filled**, because that is what the reference sends for a character with no
    // progress: 31 words each for the two hard-mode lists, and nothing for the two normal-mode ones.
    // An empty list is not the same message - it is an array the client can index past the end of.
    frames.push(game::map_update_unlocked_list([
        &[][..],
        &[],
        UNLOCKED_MAP_ZEROS,
        UNLOCKED_MAP_ZEROS,
        &[],
    ]));
    frames.push(game::instance_loaded(PLAYER_TEAM_TOKEN));

    // Skills and attributes, then factions, then the attribute spread. The profession values are the
    // reference's own for its default character, read off its send log: primary 1 (Warrior) and one
    // unlocked-profession bit.
    frames.push(game::agent_update_attribute_points(agent_id, 50, 50));
    frames.push(game::player_update_profession(
        agent_id,
        traits.profession,
        0,
        0,
    ));
    frames.push(game::player_update_unlocked_professions(
        agent_id,
        traits.unlocked_professions,
    ));
    frames.push(game::skillbar_update(agent_id, &[0; 8], &[0; 8], 1));
    frames.push(game::update_unlocked_skills(&[]));
    frames.push(game::character_update_factions(
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ));
    frames.push(game::agent_update_attributes(agent_id, &[0; 42]));
    frames.push(game::world_update_load_time(
        started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32,
    ));

    // The player, the agent, and what a client needs in order to draw them. The appearance carries
    // the profession the model is built from: all-zero bits say `Profession_None`, which is not a
    // character the client can draw, so the reference's own default is used.
    // The appearance the client chose, or the reference's default for a character with no bits recorded:
    // all-zero bits say `Profession_None`, which is not a character the client can draw.
    let appearance = if traits.appearance == 0 {
        game::Appearance::REFERENCE_DEFAULT.pack()
    } else {
        traits.appearance
    };
    frames.push(game::player_create(player_id, agent_id, appearance, name));
    // The profession goes out **twice** in the reference: once with the skills and attributes, and
    // again here, immediately after the player is created and before the agent exists
    // (`GameSrv_HandleInstanceLoadRequestPlayers`: `BroadcastPlayerCreate` then
    // `SendPlayerProfession`). It is the second one that gives the freshly created player the
    // profession its model is built from - without it the client draws an agent it has no model for.
    frames.push(game::player_update_profession(
        agent_id,
        traits.profession,
        0,
        0,
    ));
    frames.push(game::world_create_agent(&game::CreateAgent {
        agent_id,
        model_id: game::CHAR_CLASS_PLAYER_BASE | player_id,
        agent_type: game::AGENT_TYPE_LIVING,
        position: standing.position,
        plane: standing.plane,
        direction: (1.0, 0.0),
        speed_base: game::PLAYER_SPEED_BASE,
        player_team_token: PLAYER_TEAM_TOKEN,
    }));
    // Health and energy in the reference's order, then the regeneration rate and the public level.
    // The reference's own values, read off its send log: energy 25, health 400, regeneration 0.0 and
    // level 0 (it never sets `energy_per_sec` or `level` on a fresh agent).
    frames.push(game::agent_property_update_int(
        game::AGENT_PROPERTY_ENERGY,
        agent_id,
        game::PLAYER_ENERGY_MAX,
    ));
    frames.push(game::agent_property_update_int(
        game::AGENT_PROPERTY_HEALTH,
        agent_id,
        game::PLAYER_HEALTH_MAX,
    ));
    frames.push(game::agent_property_update_float(
        game::AGENT_PROPERTY_ENERGY_REGEN,
        agent_id,
        0.0,
    ));
    frames.push(game::agent_property_update_int(
        game::AGENT_PROPERTY_PUBLIC_LEVEL,
        agent_id,
        0,
    ));
    frames.push(game::agent_initial_effects(agent_id, 0));
    frames.push(game::update_agent_visual_equipment(
        agent_id,
        inventory.equipped_item_ids(),
    ));
    frames.push(game::agent_display_cape(agent_id, 1));
    frames.push(game::world_update_controlled_agent(agent_id, 3));

    // The party: one member, this player, and the client's own party pointer.
    frames.push(game::party_create(PARTY_ID));
    frames.push(game::party_player_add(PARTY_ID, player_id as u16, 1));
    frames.push(game::party_member_stream_end(PARTY_ID));
    frames.push(game::character_update_party(PARTY_ID, 1));

    frames.push(game::instance_load_finish());
    frames
}

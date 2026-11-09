//! The player: the character one connection drives, and the block that describes it.
//!
//! [`Player`] is the record and [`player_block`] is the character's own
//! `GAME_SMSG_INSTANCE_PLAYER_DATA_*` run; the constants below are the
//! reference's defaults for it (`PLAYER_ID`, `AGENT_ID`, `PARTY_ID`) and the profession it
//! reports when a character row carries none.

use crate::*;

/// The player: the character one connection drives, and the instance it is in.
///
/// The player as a record: the state this server owns, with the domain's verbs on it
/// (`player.GetAgentID()`).
///
/// Assembled after the token pair is believed and before the load block, so that everything the blocks
/// need - whose character this is, where it stands, what it carries - is decided in one place rather than
/// read again in the middle of a conversation.
/// Everything a player is admitted with: the facts the connection established before the load block.
///
/// A struct rather than fifteen positional arguments, for the reason `asura-protocol` and
/// `asura-rules` record (D16): the ids are small integers, so a swapped pair compiles and a client
/// draws the wrong character.
pub struct Admission {
    /// The character the transfer named.
    pub character: Uuid,
    /// The connection's player id.
    pub player_id: u32,
    /// The agent this character drives.
    pub agent_id: u32,
    /// The instance it entered.
    pub instance: Arc<Instance>,
    /// The character's name.
    pub name: String,
    /// What it carries, as read for the load block.
    pub inventory: Inventory,
    /// The login session the transfer named.
    pub session_id: Uuid,
    /// The account that session belongs to.
    pub account_id: Uuid,
    /// Where the doorway test starts watching.
    pub portal_position: (f32, f32),
    /// The creation in progress, when this is a creation instance.
    pub creation: Option<Creation>,
    /// The character's stored traits.
    pub traits: db_game::Traits,
}
pub struct Player {
    /// The character the transfer named. Every write on the way out is scoped to it.
    character: Uuid,
    /// This connection's player id, allocated when it joined. The reference allocates the same number
    /// from `srv->players` (`GmIdAllocate`) and puts it in `GAME_SMSG_PLAYER_CREATE`, every agent's
    /// `model_id`, the party messages and the chat speaker field.
    player_id: u32,
    /// The agent this character drives in its instance, as `GAME_SMSG_WORLD_CREATE_AGENT` names it.
    agent_id: u32,
    /// The instance this player is in: the map as a running object (its players, their agents and its
    /// clock), not the catalogue row that describes it. `player.instance.map` is that row.
    instance: Arc<Instance>,
    name: String,
    inventory: Inventory,
    /// The login session and account the transfer named.
    ///
    /// The reference keeps both on its player (`GmPlayer.account_id`, `.char_id`) and uses them for every
    /// bag and item query and for its transfer request (`GameSrv.c:1450-1472`). This server scoped its
    /// writes by character alone, which is enough for everything it did until a *world* had to start a
    /// travel: the transfer row is keyed by session and needs the account as well (`docs/history/gwps-parity/behavior-audit.md` #9).
    session_id: Uuid,
    account_id: Uuid,
    /// Set when this connection has been redirected to another map. Its teardown must then leave the
    /// lease alone: the seat it holds now belongs to the connection the client is about to open.
    moved: bool,
    /// The last client-reported position checked for doorways. Simulation ticks never change it:
    /// a tick can cross a wall between reports, and that movement must remain in the next check.
    portal_position: (f32, f32),
    /// The segment between finite client reports or corrections, consumed by the doorway test.
    last_step: Option<((f32, f32), (f32, f32))>,
    /// What a character-creation connection has chosen so far: `Some` when this connection is in a
    /// creation instance (map type 11), which is the reference's `MapType_CharacterCreation`.
    creation: Option<Creation>,
    /// The character's stored traits - what the load block sends instead of constants.
    traits: db_game::Traits,
    /// When the connection began, for the load time the reference reports.
    started: Instant,
}

#[allow(non_snake_case)]
impl Player {
    /// Preserve the full reported movement even when simulation ticks advanced the agent meanwhile.
    pub fn record_portal_step(&mut self, position: (f32, f32)) {
        if crate::move_trace_on() && self.portal_position != position {
            eprintln!(
                "move[portal] map {} client segment ({:.1}, {:.1}) -> ({:.1}, {:.1})",
                self.instance.map().map_id,
                self.portal_position.0,
                self.portal_position.1,
                position.0,
                position.1
            );
        }
        self.last_step =
            (self.portal_position != position).then_some((self.portal_position, position));
        self.portal_position = position;
    }

    /// Admit a character to a connection: the one way a `Player` is made.
    ///
    /// The fields the connection does not know are set here, and the ones it does arrive as facts, not
    /// as guesses: `moved` starts false because no redirect has happened, `last_step` empty because no
    /// report has been consumed, and the clock starts now.
    pub fn admit(admission: Admission) -> Player {
        Player {
            character: admission.character,
            player_id: admission.player_id,
            agent_id: admission.agent_id,
            instance: admission.instance,
            name: admission.name,
            inventory: admission.inventory,
            session_id: admission.session_id,
            account_id: admission.account_id,
            moved: false,
            portal_position: admission.portal_position,
            last_step: None,
            creation: admission.creation,
            traits: admission.traits,
            started: Instant::now(),
        }
    }
    /// The character the transfer named.
    pub fn character(&self) -> Uuid {
        self.character
    }

    /// This connection's player id.
    pub fn player_id(&self) -> u32 {
        self.player_id
    }

    /// The agent this character drives in its instance.
    pub fn agent_id(&self) -> u32 {
        self.agent_id
    }

    /// The instance this player is in, and through it the map it stands on.
    pub fn instance(&self) -> &Arc<Instance> {
        &self.instance
    }

    /// The character's name, as the client shows it.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What the character carries.
    pub fn inventory(&self) -> &Inventory {
        &self.inventory
    }

    /// Replace what the character carries, after a read or a creation write.
    pub fn set_inventory(&mut self, inventory: Inventory) {
        self.inventory = inventory;
    }

    /// The login session the transfer named.
    pub fn session_id(&self) -> Uuid {
        self.session_id
    }

    /// The account the session belongs to.
    pub fn account_id(&self) -> Uuid {
        self.account_id
    }

    /// Whether this connection has been redirected to another map.
    pub fn moved(&self) -> bool {
        self.moved
    }

    /// A redirect happened: the teardown must leave the lease alone.
    pub fn set_moved(&mut self, moved: bool) {
        self.moved = moved;
    }

    /// The last client-reported position the doorway test saw.
    pub fn portal_position(&self) -> (f32, f32) {
        self.portal_position
    }

    /// The segment between finite client reports, which the doorway test consumes once.
    pub fn take_last_step(&mut self) -> Option<((f32, f32), (f32, f32))> {
        self.last_step.take()
    }

    /// The creation in progress, when this connection is in a creation instance.
    pub fn creation(&self) -> Option<Creation> {
        self.creation
    }

    /// Record a creation choice.
    pub fn set_creation(&mut self, creation: Option<Creation>) {
        self.creation = creation;
    }

    /// The character's stored traits.
    pub fn traits(&self) -> db_game::Traits {
        self.traits
    }

    /// When the connection began.
    pub fn started(&self) -> Instant {
        self.started
    }
    /// Where the agent is **now**, from the instance's own slot: what the reference answers the spawn
    /// and players requests with (`agent->position`). After the client has walked, that is no longer
    /// where it entered.
    pub fn standing(&self) -> Standing {
        self.instance
            .standing(self.player_id)
            .unwrap_or(Standing::from_map(self.instance.map()))
    }

    /// `Player.GetAgentID()`: the agent this character drives.
    pub fn GetAgentID(&self) -> u32 {
        self.agent_id
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
        self.standing().position
    }

    /// `Player.GetAgent()`: the agent record itself, as a copy (the instance owns the original).
    pub fn GetAgent(&self) -> Option<AgentState> {
        self.instance.agent(self.player_id)
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

/// The unlocked-profession bitmap the creation screen is told, before a character exists
/// (`GAME_SMSG_PLAYER_UPDATE_UNLOCKED_PROFESSIONS`: `02 00 00 00`) - read from
/// `config/Server/defaults.json`. A real character's bitmap is written at the confirm and read from its
/// row on every load, so this is the screen's default and nothing else's.
pub fn creation_screen_unlocked_professions() -> u32 {
    asura_config::defaults()
        .map(|defaults| defaults.creation_screen_unlocked_professions())
        .unwrap_or(0)
}

/// How many UTF-16 units of a character name the confirm keeps, from
/// `config/Server/defaults.json` (`name_max_units`).
pub fn name_max_units() -> usize {
    asura_config::defaults()
        .map(|defaults| defaults.name_max_units())
        .unwrap_or(0)
}

/// Morale is a percentage around 100, and 100 is the value a live load carries for a character with no
/// morale modifier (`dump4-after`, `worldcontext.morale`). Sending 0 left the client's morale at zero.
const MORALE_FULL: u32 = 100;

/// The experience this world reports for a character. There is no column for it yet, and a character
/// that has never earned any reads 0 - but 0 and "the field was never written" look the same to the
/// owner's eye, so this stays 1 until the column exists: a non-zero number is the proof that the
/// message is carrying the field at all.
const EXPERIENCE_TEST_VALUE: u32 = 500;

/// What the player block is built from: everything a character's own messages carry.
///
/// A struct rather than eight positional arguments, for the reason `asura-rules` and
/// `asura-protocol` record (`docs/reference/decisions/layer-decisions.md`, D16): the id, the agent id and the traits are
/// all small integers, so a swapped pair compiles and a client draws the wrong character.
pub struct PlayerBlock<'a> {
    /// The character's name, as the client shows it.
    pub name: &'a str,
    /// Where it stands: the position and plane the client draws it at.
    pub standing: Standing,
    /// Its bags and items, as rows were read.
    pub inventory: &'a Inventory,
    /// When this connection started, which the client's clock display uses.
    pub started: Instant,
    /// The player id this connection was given.
    pub player_id: u32,
    /// The agent id this connection was given.
    pub agent_id: u32,
    /// The character's own row: appearance, campaign, level and the rest.
    pub traits: db_game::Traits,
    /// The unlocked-map bitmap, in the client's own encoding.
    pub unlocked: &'a [u32],
    /// The world's agent values (`config/World/Vanilla/agent_defaults.json`): the speed the agent is
    /// created with and walks at, and the health, energy and regeneration the block states.
    pub agent: &'a crate::agent_defaults::AgentDefaults,
}

/// Everything the client is told about the players in the instance, in the reference's order
/// (`GameSrv_HandleInstanceLoadRequestPlayers`).
///
/// Two of the block's fields are there for the two things it no longer takes from a constant:
/// `standing`, which places the agent where the character actually is (`WORLD_CREATE_AGENT`), and
/// `inventory`, which says what the agent is wearing (`UPDATE_AGENT_VISUAL_EQUIPMENT`) - the nine ids
/// read out of the equipped-items bag, because they belong to *this* character's items.
pub fn player_block(block: &PlayerBlock<'_>) -> Vec<Frame> {
    let PlayerBlock {
        name,
        standing,
        inventory,
        started,
        player_id,
        agent_id,
        traits,
        unlocked,
        agent,
    } = *block;
    let mut frames = Vec::with_capacity(30);

    // The PvP unlocks. The reference sends its own fixed table of 387 unlockable items
    // (`GameSrv_SendPvpItems`), which this crate carries as content (`crate::pvp_items`, layer 9),
    // and the client builds its unlockable-item table from them, so they are copied rather than
    // skipped. The message builder stays in the wire crate; only the rows moved.
    frames.push(builders::pvp_update_unlocked_skills(&[]));
    frames.push(builders::pvp_update_unlocked_heroes(&[0xFFFF_FFFF; 8]));
    for item in crate::pvp_items::PVP_ITEMS {
        frames.push(builders::pvp_item_stream_add(item.id, item.params));
    }
    frames.push(builders::pvp_item_stream_end());

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
        frames.push(builders::account_feature(feature, param1, param2));
    }

    // The unlocked-map list. The two hard-mode bitmaps are sent at their **full length, zero-filled**,
    // because that is what the reference sends for a character with no mission progress: 31 words each.
    // An empty list is not the same message - it is an array the client can index past the end of.
    //
    // **The fifth list is the instance bitmap, and it is the one that has to be right.** The client's
    // world context holds five arrays in this order - missions completed, mission bonuses, missions
    // completed hard, bonuses hard, unlocked instances - and reads an instance's unlock as bit
    // `map_id % 32` of word `map_id / 32` in the fifth (`docs/reference/game-world-model.md`). This server used to
    // send the fifth list empty, which is an unlocked array of length zero: every instance read as
    // locked, and the world map had nothing to offer. `unlocked` is the character's reached instances
    // (`map::unlock_bitmap`), and the instance it is standing in is always among them.
    frames.push(builders::map_update_unlocked_list([
        &[][..],
        &[],
        crate::player::UNLOCKED_MAP_ZEROS,
        crate::player::UNLOCKED_MAP_ZEROS,
        unlocked,
    ]));
    frames.push(builders::instance_loaded(PLAYER_TEAM_TOKEN));

    // Skills and attributes, then factions, then the attribute spread. The profession values are the
    // reference's own for its default character, read off its send log: primary 1 (Warrior) and one
    // unlocked-profession bit.
    frames.push(builders::agent_update_attribute_points(agent_id, 50, 50));
    frames.push(builders::player_update_profession(
        agent_id,
        traits.profession,
        0,
        0,
    ));
    frames.push(builders::player_update_unlocked_professions(
        agent_id,
        traits.unlocked_professions,
    ));
    frames.push(builders::skillbar_update(agent_id, &[0; 8], &[0; 8], 1));
    frames.push(builders::update_unlocked_skills(&[]));
    // The character record's own numbers, in the reference's order (`GameSrv_UpdatePlayerFactions`).
    // This used to be thirteen zeros, and thirteen zeros is what the client showed: level 0,
    // experience 0, morale 0. A live load leaves level 13, experience 68113 and morale 100 in the
    // client's record for a level-13 character (`dump4-after`, worldcontext.level/experience/morale),
    // and `characters.level` is where our level comes from. Experience and skill points have no column
    // yet - a freshly created character has neither - so they go out as zero rather than as absent.
    frames.push(builders::character_update_factions(
        &builders::CharacterProgress {
            // 500 until a column holds experience, so the sheet shows it working.
            experience: EXPERIENCE_TEST_VALUE,
            level: u32::from(traits.level),
            morale: MORALE_FULL,
            // Faction, skill points and their earned totals have no column yet.
            ..builders::CharacterProgress::default()
        },
    ));
    // A live load sends these two immediately after the faction block, once per player: the visibility
    // flags and the player's own flags word (`recordings/asura/world/load-layers-20261031.jsonl`,
    // headers 00EF then 003C). Without them the client's player record carries no flags and its
    // inventory display-mode column has nothing for any slot - the structures exist but stay empty.
    frames.push(builders::char_visibility_flags(0xFFFF_FFFF, 0xFFFF_FFFF));
    frames.push(builders::player_update_flags(player_id as u16, 0, 7));
    frames.push(builders::agent_update_attributes(agent_id, &[0; 42]));
    frames.push(builders::world_update_load_time(
        started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32,
    ));

    // The player, the agent, and what a client needs in order to draw them. The appearance carries
    // the profession the model is built from: all-zero bits say `Profession_None`, which is not a
    // character the client can draw, so the reference's own default is used.
    // The appearance the client chose, or the reference's default for a character with no bits recorded:
    // all-zero bits say `Profession_None`, which is not a character the client can draw.
    let appearance = if traits.appearance == 0 {
        // The look for a character with no bits of its own
        // (`config/World/Vanilla/character_defaults.json`, `appearance`): all-zero bits are
        // `Profession_None`, which the client cannot draw.
        crate::character_defaults::current()
            .map(|defaults| defaults.appearance().pack())
            .unwrap_or_default()
    } else {
        traits.appearance
    };
    frames.push(builders::player_create(
        player_id, agent_id, appearance, name,
    ));
    // The profession goes out **twice** in the reference: once with the skills and attributes, and
    // again here, immediately after the player is created and before the agent exists
    // (`GameSrv_HandleInstanceLoadRequestPlayers`: `BroadcastPlayerCreate` then
    // `SendPlayerProfession`). It is the second one that gives the freshly created player the
    // profession its model is built from - without it the client draws an agent it has no model for.
    frames.push(builders::player_update_profession(
        agent_id,
        traits.profession,
        0,
        0,
    ));
    frames.push(builders::world_create_agent(&builders::CreateAgent {
        agent_id,
        model_id: builders::CHAR_CLASS_PLAYER_BASE | player_id,
        agent_type: builders::AGENT_TYPE_LIVING,
        position: standing.position,
        plane: standing.plane,
        direction: standing.direction,
        speed_base: agent.speed(),
        player_team_token: PLAYER_TEAM_TOKEN,
    }));
    // The agent's own morale record, which the party panel's morale icon is drawn from. A live server
    // sends one per body at load with 100, the unmodified value; with the record left unset the client
    // paints a morale penalty the character does not have.
    frames.push(builders::agent_update_morale(agent_id, MORALE_FULL));
    // Health and energy in the reference's order, then the regeneration rate and the public level.
    // The numbers are the pack's (`config/World/Vanilla/agent_defaults.json`); the reference's own
    // send log shows energy 25, health 400, regeneration 0.0 and level 0 on a fresh agent.
    frames.push(builders::agent_property_update_int(
        builders::AGENT_PROPERTY_ENERGY,
        agent_id,
        agent.energy(),
    ));
    frames.push(builders::agent_property_update_int(
        builders::AGENT_PROPERTY_HEALTH,
        agent_id,
        agent.health(),
    ));
    frames.push(builders::agent_property_update_float(
        builders::AGENT_PROPERTY_ENERGY_REGEN,
        agent_id,
        agent.energy_regen(),
    ));
    // The agent's level, which the party window draws. It is not a field of the create message: it
    // arrives as agent int-property 36 (`PublicLevel`), which is what the client stores in
    // `AgentLiving.level` - measured on our own client as 0 while this sent 0, and Rurik's own reading
    // gives property 36 as the level on this exact opcode (`studies/character/FINDINGS.md`).
    frames.push(builders::agent_property_update_int(
        builders::AGENT_PROPERTY_PUBLIC_LEVEL,
        agent_id,
        u32::from(traits.level),
    ));
    frames.push(builders::agent_initial_effects(agent_id, 0));
    frames.push(builders::update_agent_visual_equipment(
        agent_id,
        inventory.equipped_item_ids(),
    ));
    frames.push(builders::agent_display_cape(agent_id, 1));
    frames.push(builders::world_update_controlled_agent(agent_id, 3));

    // The party: one member, this player, and the client's own party pointer.
    frames.push(builders::party_create(PARTY_ID));
    frames.push(builders::party_player_add(PARTY_ID, player_id as u16, 1));
    frames.push(builders::party_member_stream_end(PARTY_ID));
    frames.push(builders::character_update_party(PARTY_ID, 1));

    frames.push(builders::instance_load_finish());
    frames
}

// ---- the instances a character has reached, as the client addresses them ---------------------

/// A zero-filled bitmap of the length the reference uses for a character with no progress: 31 words.
///
/// Copied from its trace, not chosen: `GAME_SMSG_MAP_UPDATE_UNLOCKED_LIST` there carries two lists of
/// 31 zero dwords (`02 00 00 00` twice, then 31 words of zeros, twice more). Sending the lists empty
/// instead changes the *length* of a bitmap the client indexes by map id.
pub const UNLOCKED_MAP_ZEROS: &[u32] = &[0u32; 31];

/// The instance bitmap a character's reached instances become, in the client's own addressing.
///
/// **Read from the client's world context** (`docs/reference/game-world-model.md`): the bitmap is an array of
/// `u32` where instance `n` is bit `n % 32` of word `n / 32`. 31 words covers instances 0 to 991,
/// which contains the reference's own `MapId_Count` sentinel of 877 (`MAP_ID_COUNT`), so a character
/// can never outgrow the list the reference sends.
///
/// An id outside that range, or a negative one, is dropped rather than growing the list: the width is
/// the message's, and an instance the client cannot address is not made addressable by a longer array.
pub fn unlock_bitmap(instances: &[i32]) -> Vec<u32> {
    let mut words = UNLOCKED_MAP_ZEROS.to_vec();
    for &map_id in instances {
        let index = usize::try_from(map_id).ok().map(|id| id / 32);
        if let Some(word) = index.filter(|index| *index < words.len()) {
            words[word] |= 1 << (map_id.rem_euclid(32) as u32);
        }
    }
    words
}

// ---- the client asks who else is in the instance ------------------------------------------

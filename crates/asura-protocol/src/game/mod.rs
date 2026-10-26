//! The game protocol: what a world server sends to a client that has just been placed in a map.
//!
//! `docs/03-protocol.md` section 11 records the order. This module owns the **messages**, one builder
//! per message, each written to the byte layout of the reference's packed struct (`GameMsg.h`) and
//! each tested against the size the reference's own trace records for it.
//!
//! Two rules that come from the reference and are not negotiable:
//!
//! * **The header is two bytes, and the direction is the top bit.** Client-to-server is `0x8000 | n`,
//!   server-to-client is `n` (GWPS `opcodes.h`). A world server that swaps them sends a client a
//!   message it reads as a different one.
//! * **A counted field is a `u16` count followed by exactly that many units**, with no padding -- the
//!   same rule as the auth path (`client::frame`). The reference's structs declare wider counters
//!   (`uint32_t n_name`), which is why `sizeof(struct)` is not the wire size and why these builders
//!   write the count themselves instead of mirroring a struct.
//!
//! Everything here is server-to-client. The client-to-server messages a world server must *read* are
//! framed by the generated game table ([`super::client::game_tables`]) through
//! [`super::client::Frame::decode_game_message`].

/// `GAME_SMSG_INSTANCE_LOAD_HEAD` (`0x017C`): 6 bytes. The first message a client receives on the
/// game connection.
pub const INSTANCE_LOAD_HEAD: u16 = 0x017C;
/// `GAME_SMSG_INSTANCE_PLAYER_DATA_START` (`0x0186`): 2 bytes.
pub const INSTANCE_PLAYER_DATA_START: u16 = 0x0186;
/// `GAME_SMSG_INSTANCE_LOAD_PLAYER_NAME` (`0x017D`): `2 + 2 + 2n`.
pub const INSTANCE_LOAD_PLAYER_NAME: u16 = 0x017D;
/// `GAME_SMSG_INSTANCE_LOAD_INFO` (`0x019A`): 16 bytes.
pub const INSTANCE_LOAD_INFO: u16 = 0x019A;
/// `GAME_SMSG_INSTANCE_LOAD_SPAWN_POINT` (`0x0196`): 26 bytes.
pub const INSTANCE_LOAD_SPAWN_POINT: u16 = 0x0196;
/// `GAME_SMSG_INSTANCE_LOADED` (`0x00F2`): 6 bytes.
pub const INSTANCE_LOADED: u16 = 0x00F2;
/// `GAME_SMSG_INSTANCE_LOAD_FINISH` (`0x018E`): 2 bytes. The client leaves the loading screen on it.
pub const INSTANCE_LOAD_FINISH: u16 = 0x018E;
/// `GAME_SMSG_INSTANCE_MANIFEST_PHASE` (`0x0199`): 3 bytes.
pub const INSTANCE_MANIFEST_PHASE: u16 = 0x0199;
/// `GAME_SMSG_INSTANCE_MANIFEST_DONE` (`0x0198`): 9 bytes.
pub const INSTANCE_MANIFEST_DONE: u16 = 0x0198;

/// `GAME_SMSG_MAP_UPDATE_CURRENT` (`0x0099`), the **login** shape: 5 bytes, `map_id` is a `u16`.
///
/// The redirect shape of the same opcode is 12 bytes with a `u32` map id. Both exist in the reference
/// (`GameMsg.h`, `MapUpdateCurrent` comment) and getting them the wrong way round is the trap that
/// costs a client error 007.
pub const MAP_UPDATE_CURRENT: u16 = 0x0099;
/// `GAME_SMSG_READY_FOR_MAP_SPAWN` (`0x01AC`): 6 bytes.
pub const READY_FOR_MAP_SPAWN: u16 = 0x01AC;
/// `GAME_SMSG_MAP_UPDATE_UNLOCKED_LIST` (`0x0094`): five counted lists of dwords.
pub const MAP_UPDATE_UNLOCKED_LIST: u16 = 0x0094;

/// `GAME_SMSG_ITEM_STREAM_CREATE` (`0x0144`): 5 bytes.
pub const ITEM_STREAM_CREATE: u16 = 0x0144;
/// `GAME_SMSG_INVENTORY_CREATE_BAG` (`0x013F`): 13 bytes.
pub const INVENTORY_CREATE_BAG: u16 = 0x013F;
/// `GAME_SMSG_ITEM_SET_ACTIVE_WEAPON_SET` (`0x0148`): 5 bytes.
pub const ITEM_SET_ACTIVE_WEAPON_SET: u16 = 0x0148;
/// `GAME_SMSG_ITEM_WEAPON_SET` (`0x0147`): 13 bytes, sent once per weapon slot.
pub const ITEM_WEAPON_SET: u16 = 0x0147;
/// `GAME_SMSG_UPDATE_GOLD_STORAGE` (`0x0141`): 8 bytes.
pub const UPDATE_GOLD_STORAGE: u16 = 0x0141;
/// `GAME_SMSG_CHARACTER_FACTION_MAX_KURZICK` (`0x00EA`): 6 bytes.
pub const FACTION_MAX_KURZICK: u16 = 0x00EA;
/// `GAME_SMSG_CHARACTER_FACTION_MAX_LUXON` (`0x00EB`): 6 bytes.
pub const FACTION_MAX_LUXON: u16 = 0x00EB;
/// `GAME_SMSG_CHARACTER_FACTION_MAX_BALTHAZAR` (`0x00EC`): 6 bytes.
pub const FACTION_MAX_BALTHAZAR: u16 = 0x00EC;
/// `GAME_SMSG_CHARACTER_FACTION_MAX_IMPERIAL` (`0x00ED`): 6 bytes.
pub const FACTION_MAX_IMPERIAL: u16 = 0x00ED;
/// `GAME_SMSG_CHARACTER_UPDATE_INFO` (`0x0030`): `28 + 2n`.
pub const CHARACTER_UPDATE_INFO: u16 = 0x0030;
/// `GAME_SMSG_HARD_MODE_UNLOCKED` (`0x0070`): 3 bytes.
pub const HARD_MODE_UNLOCKED: u16 = 0x0070;
/// `GAME_SMSG_CHARACTER_UPDATE_FACTIONS` (`0x00E9`): 62 bytes.
pub const CHARACTER_UPDATE_FACTIONS: u16 = 0x00E9;

/// `GAME_SMSG_PVP_UPDATE_UNLOCKED_SKILLS` (`0x001D`): `4 + 4n`.
pub const PVP_UPDATE_UNLOCKED_SKILLS: u16 = 0x001D;
/// `GAME_SMSG_PVP_UPDATE_UNLOCKED_HEROES` (`0x0018`): `4 + 4n`.
pub const PVP_UPDATE_UNLOCKED_HEROES: u16 = 0x0018;
/// `GAME_SMSG_PVP_ITEM_STREAM_ADD` (`0x001A`): `5 + 4n`.
pub const PVP_ITEM_STREAM_ADD: u16 = 0x001A;
/// `GAME_SMSG_PVP_ITEM_STREAM_END` (`0x001B`): 2 bytes.
pub const PVP_ITEM_STREAM_END: u16 = 0x001B;
/// `GAME_SMSG_ACCOUNT_FEATURE` (`0x000F`): 8 bytes.
pub const ACCOUNT_FEATURE: u16 = 0x000F;

/// `GAME_SMSG_AGENT_UPDATE_ATTRIBUTE_POINTS` (`0x0037`): 8 bytes.
pub const AGENT_UPDATE_ATTRIBUTE_POINTS: u16 = 0x0037;
/// `GAME_SMSG_PLAYER_UPDATE_PROFESSION` (`0x00B7`): 9 bytes.
pub const PLAYER_UPDATE_PROFESSION: u16 = 0x00B7;
/// `GAME_SMSG_PLAYER_UPDATE_UNLOCKED_PROFESSIONS` (`0x00B6`): 10 bytes.
pub const PLAYER_UPDATE_UNLOCKED_PROFESSIONS: u16 = 0x00B6;
/// `GAME_SMSG_SKILLBAR_UPDATE` (`0x00DA`): `11 + 4n_skills + 4n_masks + 1`.
pub const SKILLBAR_UPDATE: u16 = 0x00DA;
/// `GAME_SMSG_UPDATE_UNLOCKED_SKILLS` (`0x00DB`): `4 + 4n`.
pub const UPDATE_UNLOCKED_SKILLS: u16 = 0x00DB;
/// `GAME_SMSG_AGENT_UPDATE_ATTRIBUTES` (`0x003A`): `8 + 4n`.
pub const AGENT_UPDATE_ATTRIBUTES: u16 = 0x003A;
/// `GAME_SMSG_WORLD_UPDATE_LOAD_TIME` (`0x001F`): 6 bytes.
pub const WORLD_UPDATE_LOAD_TIME: u16 = 0x001F;

/// `GAME_SMSG_PLAYER_CREATE` (`0x0059`): `25 + 2n`.
pub const PLAYER_CREATE: u16 = 0x0059;
/// `GAME_SMSG_WORLD_CREATE_AGENT` (`0x0020`): 99 bytes, fixed.
pub const WORLD_CREATE_AGENT: u16 = 0x0020;
/// `GAME_SMSG_AGENT_PROPERTY_UPDATE_INT` (`0x009F`): 14 bytes.
pub const AGENT_PROPERTY_UPDATE_INT: u16 = 0x009F;
/// `GAME_SMSG_AGENT_PROPERTY_UPDATE_FLOAT` (`0x00A2`): 14 bytes.
pub const AGENT_PROPERTY_UPDATE_FLOAT: u16 = 0x00A2;
/// `GAME_SMSG_AGENT_INITIAL_EFFECTS` (`0x00F0`): 10 bytes.
pub const AGENT_INITIAL_EFFECTS: u16 = 0x00F0;
/// `GAME_SMSG_UPDATE_AGENT_VISUAL_EQUIPMENT` (`0x006E`): 42 bytes.
pub const UPDATE_AGENT_VISUAL_EQUIPMENT: u16 = 0x006E;
/// `GAME_SMSG_AGENT_DISPLAY_CAPE` (`0x0048`): 7 bytes.
pub const AGENT_DISPLAY_CAPE: u16 = 0x0048;
/// `GAME_SMSG_WORLD_UPDATE_CONTROLLED_AGENT` (`0x0022`): 10 bytes.
pub const WORLD_UPDATE_CONTROLLED_AGENT: u16 = 0x0022;

/// `GAME_SMSG_PARTY_CREATE` (`0x01D3`): 4 bytes.
pub const PARTY_CREATE: u16 = 0x01D3;
/// `GAME_SMSG_PARTY_PLAYER_ADD` (`0x01CC`): 7 bytes.
pub const PARTY_PLAYER_ADD: u16 = 0x01CC;
/// `GAME_SMSG_PARTY_MEMBER_STREAM_END` (`0x01D4`): 4 bytes.
pub const PARTY_MEMBER_STREAM_END: u16 = 0x01D4;
/// `GAME_SMSG_CHARACTER_UDPATE_PARTY` (`0x01B3`, the reference's own spelling): 5 bytes.
pub const CHARACTER_UPDATE_PARTY: u16 = 0x01B3;

/// `GAME_SMSG_WORLD_SIMULATION_TICK` (`0x001E`): 6 bytes.
pub const WORLD_SIMULATION_TICK: u16 = 0x001E;
/// `GAME_SMSG_PING_REQUEST` (`0x000C`): 2 bytes.
pub const PING_REQUEST: u16 = 0x000C;
/// `GAME_SMSG_PING_REPLY` (`0x000D`): 6 bytes.
pub const PING_REPLY: u16 = 0x000D;

/// The agent/character property ids this module needs (`GmAgentProperties.h`).
pub const AGENT_PROPERTY_PUBLIC_LEVEL: u32 = 36;
/// See [`AGENT_PROPERTY_PUBLIC_LEVEL`].
pub const AGENT_PROPERTY_ENERGY: u32 = 41;
/// See [`AGENT_PROPERTY_PUBLIC_LEVEL`].
pub const AGENT_PROPERTY_HEALTH: u32 = 42;
/// See [`AGENT_PROPERTY_PUBLIC_LEVEL`].
pub const AGENT_PROPERTY_ENERGY_REGEN: u32 = 43;

/// `GameSrv_CreatePlayerAgent`'s defaults: a player agent is a living agent that moves at 288 units a
/// second, with 400 health and 25 energy. The reference hardcodes them and so does this.
pub const PLAYER_SPEED_BASE: f32 = 288.0;
/// See [`PLAYER_SPEED_BASE`].
pub const PLAYER_HEALTH_MAX: u32 = 400;
/// See [`PLAYER_SPEED_BASE`].
pub const PLAYER_ENERGY_MAX: u32 = 25;
/// See [`PLAYER_SPEED_BASE`]; `energy_per_sec`.
pub const PLAYER_ENERGY_REGEN: f32 = 0.033;

/// `CHAR_CLASS_PLAYER_BASE`: the top nibble of a player agent's model id. A player agent's model id
/// is this bit-or'd with the player id (`GameSrv_CreatePlayerAgent`).
pub const CHAR_CLASS_PLAYER_BASE: u32 = 0x3000_0000;
/// `AgentType_Living`: the agent type of a player (`GmAgent.h`).
pub const AGENT_TYPE_LIVING: u8 = 1;

/// A message body being built, with the header first.
struct Builder {
    bytes: Vec<u8>,
}

impl Builder {
    fn new(header: u16) -> Self {
        let mut bytes = Vec::with_capacity(128);
        bytes.extend_from_slice(&header.to_le_bytes());
        Builder { bytes }
    }

    fn u8(mut self, value: u8) -> Self {
        self.bytes.push(value);
        self
    }

    fn u16(mut self, value: u16) -> Self {
        self.bytes.extend_from_slice(&value.to_le_bytes());
        self
    }

    fn u32(mut self, value: u32) -> Self {
        self.bytes.extend_from_slice(&value.to_le_bytes());
        self
    }

    fn f32(mut self, value: f32) -> Self {
        self.bytes.extend_from_slice(&value.to_le_bytes());
        self
    }

    /// A `u16` count and the UTF-16 units of `text`, with no padding.
    ///
    /// The reference's structs declare a `uint32_t` count and a fixed array; the wire carries the
    /// count as two bytes and only the units actually used. The trace settles it: a five-character
    /// name is `2 + 2 + 10`.
    fn text(mut self, text: &str, capacity: usize) -> Self {
        let units: Vec<u16> = text.encode_utf16().take(capacity).collect();
        self.bytes
            .extend_from_slice(&(units.len() as u16).to_le_bytes());
        for unit in units {
            self.bytes.extend_from_slice(&unit.to_le_bytes());
        }
        self
    }

    /// A `u16` count followed by that many dwords.
    fn dwords(mut self, values: &[u32]) -> Self {
        self.bytes
            .extend_from_slice(&(values.len() as u16).to_le_bytes());
        for value in values {
            self.bytes.extend_from_slice(&value.to_le_bytes());
        }
        self
    }

    fn frame(self) -> crate::client::Frame {
        let header = u16::from_le_bytes([self.bytes[0], self.bytes[1]]);
        crate::client::Frame::new(header, self.bytes[2..].to_vec())
    }
}

/// `GAME_SMSG_INSTANCE_LOAD_HEAD`: which campaigns this account may enter.
///
/// The reference sends `0x31 | Prophecies | Factions | Nightfall` for both the PvE and the PvP mask,
/// i.e. `0x3F`. The two trailing bytes are zero.
pub fn instance_load_head(pve_unlocked: u8, pvp_unlocked: u8) -> crate::client::Frame {
    Builder::new(INSTANCE_LOAD_HEAD)
        .u8(pve_unlocked)
        .u8(pvp_unlocked)
        .u8(0)
        .u8(0)
        .frame()
}

/// `GAME_SMSG_INSTANCE_LOAD_HEAD` with the reference's own flags.
pub fn instance_load_head_default() -> crate::client::Frame {
    instance_load_head(0x3F, 0x3F)
}

/// `GAME_SMSG_INSTANCE_PLAYER_DATA_START`: a bare header; it opens the player-data stream.
pub fn instance_player_data_start() -> crate::client::Frame {
    Builder::new(INSTANCE_PLAYER_DATA_START).frame()
}

/// `GAME_SMSG_INSTANCE_LOAD_PLAYER_NAME`: the character's name, as a counted UTF-16 string.
pub fn instance_load_player_name(name: &str) -> crate::client::Frame {
    Builder::new(INSTANCE_LOAD_PLAYER_NAME)
        .text(name, 20)
        .frame()
}

/// `GAME_SMSG_INSTANCE_LOAD_INFO`: which agent the client controls, in which map.
///
/// `is_explorable` is hardcoded to 1 in the reference with the comment `// what to put here?`, and the
/// reference's behaviour is the floor this project builds on, so it is a parameter here and the
/// caller passes 1 for now.
pub fn instance_load_info(
    agent: u32,
    map_id: u16,
    is_explorable: u8,
    district: u32,
    language: u8,
    is_observer: u8,
) -> crate::client::Frame {
    Builder::new(INSTANCE_LOAD_INFO)
        .u32(agent)
        .u16(map_id)
        .u8(is_explorable)
        .u32(district)
        .u8(language)
        .u8(is_observer)
        .u8(0)
        .frame()
}

/// `GAME_SMSG_INSTANCE_LOAD_SPAWN_POINT`: where to put the character, and which map blob to load.
pub fn instance_load_spawn_point(
    map_file_id: u32,
    x: f32,
    y: f32,
    plane: u16,
) -> crate::client::Frame {
    let mut builder = Builder::new(INSTANCE_LOAD_SPAWN_POINT)
        .u32(map_file_id)
        .f32(x)
        .f32(y)
        .u16(plane)
        .u8(0) // unk0
        .u8(0); // is_cinematic
    builder.bytes.extend_from_slice(&[0u8; 8]);
    builder.frame()
}

/// `GAME_SMSG_INSTANCE_LOADED`: the player's team token, which the client keeps for the session.
pub fn instance_loaded(player_team_token: u32) -> crate::client::Frame {
    Builder::new(INSTANCE_LOADED).u32(player_team_token).frame()
}

/// `GAME_SMSG_INSTANCE_LOAD_FINISH`: the last message of the load; the client stops loading on it.
pub fn instance_load_finish() -> crate::client::Frame {
    Builder::new(INSTANCE_LOAD_FINISH).frame()
}

/// `GAME_SMSG_INSTANCE_MANIFEST_PHASE`: one stage of the download manifest.
pub fn instance_manifest_phase(phase: u8) -> crate::client::Frame {
    Builder::new(INSTANCE_MANIFEST_PHASE).u8(phase).frame()
}

/// `GAME_SMSG_INSTANCE_MANIFEST_DONE`: a manifest stage finished, naming the map it covered.
pub fn instance_manifest_done(phase: u8, map_id: u16) -> crate::client::Frame {
    Builder::new(INSTANCE_MANIFEST_DONE)
        .u8(phase)
        .u16(map_id)
        .u32(0)
        .frame()
}

/// `GAME_SMSG_ITEM_STREAM_CREATE`: opens the item stream on which the inventory follows.
pub fn item_stream_create(stream: u16, is_hero: u8) -> crate::client::Frame {
    Builder::new(ITEM_STREAM_CREATE)
        .u16(stream)
        .u8(is_hero)
        .frame()
}

/// `GAME_SMSG_ITEM_SET_ACTIVE_WEAPON_SET`: which weapon set is in hand.
pub fn item_set_active_weapon_set(stream: u16, slot: u8) -> crate::client::Frame {
    Builder::new(ITEM_SET_ACTIVE_WEAPON_SET)
        .u16(stream)
        .u8(slot)
        .frame()
}

/// `GAME_SMSG_INVENTORY_CREATE_BAG`: one bag of the inventory.
///
/// A client that gets no bag at all has no inventory model to put the weapon sets and the equipped
/// items into; the reference always creates six (`GameSrv_CreateDefaultBags`), and this builder exists
/// so that "no bags" is never sent by accident.
pub fn inventory_create_bag(
    stream: u16,
    bag_type: u8,
    bag_model_id: u8,
    bag_id: u16,
    slot_count: u8,
    assoc_item_id: u32,
) -> crate::client::Frame {
    Builder::new(INVENTORY_CREATE_BAG)
        .u16(stream)
        .u8(bag_type)
        .u8(bag_model_id)
        .u16(bag_id)
        .u8(slot_count)
        .u32(assoc_item_id)
        .frame()
}

/// The bags a character has when it has never been loaded before (`GameSrv_CreateDefaultBags` and the
/// `GmBag_Init*` functions), in the order the reference sends them — its bag array's order, which is
/// not the order the ids were handed out in.
pub const DEFAULT_BAGS: [(u16, u8, u8, u8); 6] = [
    // (bag id, bag model id, bag type, slot count)
    (1, 0, 1, 20), // backpack
    (2, 5, 5, 41), // material storage
    (6, 6, 3, 12), // unclaimed items
    (3, 7, 4, 25), // storage 1
    (4, 8, 4, 25), // storage 2
    (5, 21, 2, 9), // equipped items
];

/// `GAME_SMSG_ITEM_WEAPON_SET`: the items in one weapon slot. The reference sends four of these, one
/// per slot, all empty.
pub fn item_weapon_set(stream: u16, slot: u8, leadhand: u32, offhand: u32) -> crate::client::Frame {
    Builder::new(ITEM_WEAPON_SET)
        .u16(stream)
        .u8(slot)
        .u32(leadhand)
        .u32(offhand)
        .frame()
}

/// `GAME_SMSG_UPDATE_GOLD_STORAGE`: how much gold is in storage.
pub fn update_gold_storage(stream: u16, gold: u32) -> crate::client::Frame {
    Builder::new(UPDATE_GOLD_STORAGE)
        .u16(stream)
        .u32(gold)
        .frame()
}

/// A `GAME_SMSG_CHARACTER_FACTION_MAX_*` message: the cap on one faction.
pub fn faction_max(header: u16, max_faction: u32) -> crate::client::Frame {
    Builder::new(header).u32(max_faction).frame()
}

/// `GAME_SMSG_CHARACTER_UPDATE_INFO`: the character's name and six numbers the reference sets to
/// `{0, 0, 1000, 0, 0, 0}` with an **empty** name. The name is passed in so a caller may fill it, and
/// the reference's own choice (empty) is what the world server uses today.
pub fn character_update_info(name: &str) -> crate::client::Frame {
    Builder::new(CHARACTER_UPDATE_INFO)
        .text(name, 32)
        .u32(0)
        .u32(0)
        .u32(1000)
        .u32(0)
        .u32(0)
        .u32(0)
        .frame()
}

/// `GAME_SMSG_MAP_UPDATE_CURRENT`, login shape: the map the client is now in, `u16` and a zero byte.
pub fn map_update_current(map_id: u16) -> crate::client::Frame {
    Builder::new(MAP_UPDATE_CURRENT).u16(map_id).u8(0).frame()
}

/// `GAME_SMSG_READY_FOR_MAP_SPAWN`: the client may place the agents.
pub fn ready_for_map_spawn() -> crate::client::Frame {
    Builder::new(READY_FOR_MAP_SPAWN).u32(0).frame()
}

/// `GAME_SMSG_MAP_UPDATE_UNLOCKED_LIST`: five counted lists — completed missions and bonuses on
/// normal and hard mode, and the unlocked maps.
pub fn map_update_unlocked_list(lists: [&[u32]; 5]) -> crate::client::Frame {
    let mut builder = Builder::new(MAP_UPDATE_UNLOCKED_LIST);
    for list in lists {
        builder = builder.dwords(list);
    }
    builder.frame()
}

/// `GAME_SMSG_HARD_MODE_UNLOCKED`.
pub fn hard_mode_unlocked(unlocked: u8) -> crate::client::Frame {
    Builder::new(HARD_MODE_UNLOCKED).u8(unlocked).frame()
}

/// `GAME_SMSG_CHARACTER_UPDATE_FACTIONS`: fifteen dwords of character and account progress, in the
/// reference's order (`GameSrv_UpdatePlayerFactions`).
#[allow(clippy::too_many_arguments)]
pub fn character_update_factions(
    experience: u32,
    current_kurzick: u32,
    total_earned_kurzick: u32,
    current_luxon: u32,
    total_earned_luxon: u32,
    current_imperial: u32,
    total_earned_imperial: u32,
    level: u32,
    morale: u32,
    current_balthazar: u32,
    total_earned_balthazar: u32,
    current_skill_points: u32,
    total_earned_skill_points: u32,
) -> crate::client::Frame {
    Builder::new(CHARACTER_UPDATE_FACTIONS)
        .u32(experience)
        .u32(current_kurzick)
        .u32(total_earned_kurzick)
        .u32(current_luxon)
        .u32(total_earned_luxon)
        .u32(current_imperial)
        .u32(total_earned_imperial)
        .u32(0) // unk_faction4
        .u32(0) // unk_faction5
        .u32(level)
        .u32(morale)
        .u32(current_balthazar)
        .u32(total_earned_balthazar)
        .u32(current_skill_points)
        .u32(total_earned_skill_points)
        .frame()
}

/// `GAME_SMSG_PVP_UPDATE_UNLOCKED_SKILLS`: the skills unlocked for PvP.
pub fn pvp_update_unlocked_skills(skills: &[u32]) -> crate::client::Frame {
    Builder::new(PVP_UPDATE_UNLOCKED_SKILLS)
        .dwords(skills)
        .frame()
}

/// `GAME_SMSG_PVP_UPDATE_UNLOCKED_HEROES`: a bitmap of unlocked PvP heroes. The reference sends eight
/// words of `0xFFFFFFFF`.
pub fn pvp_update_unlocked_heroes(bitmap: &[u32]) -> crate::client::Frame {
    Builder::new(PVP_UPDATE_UNLOCKED_HEROES)
        .dwords(bitmap)
        .frame()
}

/// `GAME_SMSG_PVP_ITEM_STREAM_ADD`: one unlockable PvP item and its parameters.
pub fn pvp_item_stream_add(item_id: u16, params: &[u32]) -> crate::client::Frame {
    Builder::new(PVP_ITEM_STREAM_ADD)
        .u16(item_id)
        .dwords(params)
        .frame()
}

/// `GAME_SMSG_PVP_ITEM_STREAM_END`.
pub fn pvp_item_stream_end() -> crate::client::Frame {
    Builder::new(PVP_ITEM_STREAM_END).frame()
}

/// `GAME_SMSG_ACCOUNT_FEATURE`: one account entitlement, `feature_id` and two parameters.
pub fn account_feature(feature_id: u16, param1: u16, param2: u16) -> crate::client::Frame {
    Builder::new(ACCOUNT_FEATURE)
        .u16(feature_id)
        .u16(param1)
        .u16(param2)
        .frame()
}

/// `GAME_SMSG_AGENT_UPDATE_ATTRIBUTE_POINTS`: unspent and spent attribute points.
pub fn agent_update_attribute_points(
    agent: u32,
    unused_points: u8,
    used_points: u8,
) -> crate::client::Frame {
    Builder::new(AGENT_UPDATE_ATTRIBUTE_POINTS)
        .u32(agent)
        .u8(unused_points)
        .u8(used_points)
        .frame()
}

/// `GAME_SMSG_PLAYER_UPDATE_PROFESSION`: the agent's primary and secondary profession.
pub fn player_update_profession(
    agent: u32,
    primary: u8,
    secondary: u8,
    is_pvp: u8,
) -> crate::client::Frame {
    Builder::new(PLAYER_UPDATE_PROFESSION)
        .u32(agent)
        .u8(primary)
        .u8(secondary)
        .u8(is_pvp)
        .frame()
}

/// `GAME_SMSG_PLAYER_UPDATE_UNLOCKED_PROFESSIONS`: a bitmap of professions the character may change to.
pub fn player_update_unlocked_professions(agent: u32, unlocked: u32) -> crate::client::Frame {
    Builder::new(PLAYER_UPDATE_UNLOCKED_PROFESSIONS)
        .u32(agent)
        .u32(unlocked)
        .frame()
}

/// `GAME_SMSG_SKILLBAR_UPDATE`: the eight skills on the bar and their PvP masks.
pub fn skillbar_update(
    agent: u32,
    skills: &[u32],
    pvp_masks: &[u32],
    unk: u8,
) -> crate::client::Frame {
    let mut builder = Builder::new(SKILLBAR_UPDATE).u32(agent).dwords(skills);
    builder = builder.dwords(pvp_masks).u8(unk);
    builder.frame()
}

/// `GAME_SMSG_UPDATE_UNLOCKED_SKILLS`: the skills unlocked for PvE.
pub fn update_unlocked_skills(skills: &[u32]) -> crate::client::Frame {
    Builder::new(UPDATE_UNLOCKED_SKILLS).dwords(skills).frame()
}

/// `GAME_SMSG_AGENT_UPDATE_ATTRIBUTES`: the agent's attribute ranks, one dword each.
pub fn agent_update_attributes(agent: u32, attributes: &[u32]) -> crate::client::Frame {
    Builder::new(AGENT_UPDATE_ATTRIBUTES)
        .u32(agent)
        .dwords(attributes)
        .frame()
}

/// `GAME_SMSG_WORLD_UPDATE_LOAD_TIME`: how long this instance took to load, in milliseconds.
pub fn world_update_load_time(load_time: u32) -> crate::client::Frame {
    Builder::new(WORLD_UPDATE_LOAD_TIME).u32(load_time).frame()
}

/// `GAME_SMSG_PLAYER_CREATE`: a player exists, with its id, agent, appearance and name.
pub fn player_create(
    player_id: u32,
    agent_id: u32,
    appearance: u32,
    name: &str,
) -> crate::client::Frame {
    Builder::new(PLAYER_CREATE)
        .u32(player_id)
        .u32(agent_id)
        .u32(appearance)
        .u8(0) // unk0
        .u32(0) // unk1
        .u32(0) // unk2
        .text(name, 32)
        .frame()
}

/// The 99-byte `GAME_SMSG_WORLD_CREATE_AGENT`, field for field as the reference builds it
/// (`GameSrv_BuildCreateAgentMsg`).
///
/// `h000B = 5`, `h001E = 1`, `h0023 = 1.0` and `h0027 = 0x41400000` are the reference's own unverified
/// constants; the two `HUGE_VALF` pairs are what it sends for the unmodelled vectors. They are copied
/// rather than reasoned about, because a client that reads them differently will still read them the
/// same way as it does from GWPS.
pub fn world_create_agent(
    agent_id: u32,
    model_id: u32,
    position: (f32, f32),
    plane: u16,
    direction: (f32, f32),
    speed_base: f32,
    player_team_token: u32,
) -> crate::client::Frame {
    let mut builder = Builder::new(WORLD_CREATE_AGENT)
        .u32(agent_id)
        .u32(model_id)
        .u8(AGENT_TYPE_LIVING)
        .u8(5) // h000B
        .f32(position.0)
        .f32(position.1)
        .u16(plane)
        .f32(direction.0)
        .f32(direction.1)
        .u8(1) // h001E
        .f32(speed_base)
        .f32(1.0) // h0023
        .u32(0x4140_0000) // h0027: 12.0 reinterpreted, exactly as the reference writes it
        .u32(player_team_token)
        .u32(0) // h002F
        .u32(0) // h0033
        .u32(0) // h0037
        .u32(0) // h003B
        .u32(0) // h003F
        .f32(0.0) // h0043.x
        .f32(0.0) // h0043.y
        .f32(f32::INFINITY) // h004B.x
        .f32(f32::INFINITY) // h004B.y
        .u16(0) // h0053
        .u32(0) // h0055
        .f32(f32::INFINITY) // h0059.x
        .f32(f32::INFINITY) // h0059.y
        .u16(0); // h0061
    builder.bytes.reserve(0);
    builder.frame()
}

/// `GAME_SMSG_AGENT_PROPERTY_UPDATE_INT`: one integer property of an agent.
pub fn agent_property_update_int(prop_id: u32, agent_id: u32, value: u32) -> crate::client::Frame {
    Builder::new(AGENT_PROPERTY_UPDATE_INT)
        .u32(prop_id)
        .u32(agent_id)
        .u32(value)
        .frame()
}

/// `GAME_SMSG_AGENT_PROPERTY_UPDATE_FLOAT`: one float property of an agent.
pub fn agent_property_update_float(
    prop_id: u32,
    agent_id: u32,
    value: f32,
) -> crate::client::Frame {
    Builder::new(AGENT_PROPERTY_UPDATE_FLOAT)
        .u32(prop_id)
        .u32(agent_id)
        .f32(value)
        .frame()
}

/// `GAME_SMSG_AGENT_INITIAL_EFFECTS`: the effect bitmask an agent starts with.
pub fn agent_initial_effects(agent_id: u32, effects: u32) -> crate::client::Frame {
    Builder::new(AGENT_INITIAL_EFFECTS)
        .u32(agent_id)
        .u32(effects)
        .frame()
}

/// `GAME_SMSG_UPDATE_AGENT_VISUAL_EQUIPMENT`: the nine item ids an agent wears.
pub fn update_agent_visual_equipment(agent_id: u32, items: [u32; 9]) -> crate::client::Frame {
    let mut builder = Builder::new(UPDATE_AGENT_VISUAL_EQUIPMENT).u32(agent_id);
    for item in items {
        builder = builder.u32(item);
    }
    builder.frame()
}

/// `GAME_SMSG_AGENT_DISPLAY_CAPE`.
pub fn agent_display_cape(agent_id: u32, status: u8) -> crate::client::Frame {
    Builder::new(AGENT_DISPLAY_CAPE)
        .u32(agent_id)
        .u8(status)
        .frame()
}

/// `GAME_SMSG_WORLD_UPDATE_CONTROLLED_AGENT`: which agent this client drives. The reference sends 3 as
/// the second field and nothing reads it back.
pub fn world_update_controlled_agent(agent_id: u32, unk: u32) -> crate::client::Frame {
    Builder::new(WORLD_UPDATE_CONTROLLED_AGENT)
        .u32(agent_id)
        .u32(unk)
        .frame()
}

/// `GAME_SMSG_PARTY_CREATE`.
pub fn party_create(party_id: u16) -> crate::client::Frame {
    Builder::new(PARTY_CREATE).u16(party_id).frame()
}

/// `GAME_SMSG_PARTY_PLAYER_ADD`: one member of the party.
pub fn party_player_add(party_id: u16, player_id: u16, is_loaded: u8) -> crate::client::Frame {
    Builder::new(PARTY_PLAYER_ADD)
        .u16(party_id)
        .u16(player_id)
        .u8(is_loaded)
        .frame()
}

/// `GAME_SMSG_PARTY_MEMBER_STREAM_END`.
pub fn party_member_stream_end(party_id: u16) -> crate::client::Frame {
    Builder::new(PARTY_MEMBER_STREAM_END).u16(party_id).frame()
}

/// `GAME_SMSG_CHARACTER_UDPATE_PARTY`: which party the character is in.
pub fn character_update_party(party_id: u16, unknown: u8) -> crate::client::Frame {
    Builder::new(CHARACTER_UPDATE_PARTY)
        .u16(party_id)
        .u8(unknown)
        .frame()
}

/// `GAME_SMSG_WORLD_SIMULATION_TICK`: the world's clock. The client expects one at least every 500 ms.
pub fn world_simulation_tick(delta_ms: u32) -> crate::client::Frame {
    Builder::new(WORLD_SIMULATION_TICK).u32(delta_ms).frame()
}

/// `GAME_SMSG_PING_REQUEST`: asks the client to answer with a ping reply.
pub fn ping_request() -> crate::client::Frame {
    Builder::new(PING_REQUEST).frame()
}

/// `GAME_SMSG_PING_REPLY`: the answer to a client's ping request.
pub fn ping_reply(ping: u32) -> crate::client::Frame {
    Builder::new(PING_REPLY).u32(ping).frame()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wire size of a built frame, header included.
    fn wire(frame: &crate::client::Frame) -> usize {
        frame.encode().len()
    }

    #[test]
    fn the_fixed_size_messages_match_the_size_the_reference_sends() {
        // Every number here is a size measured from the reference's own send log
        // (docs/03-protocol.md section 11), not from a struct: `sizeof` is a maximum, the wire is not.
        assert_eq!(wire(&instance_load_head_default()), 6);
        assert_eq!(wire(&instance_player_data_start()), 2);
        assert_eq!(wire(&instance_load_info(1, 146, 1, 1, 0, 0)), 16);
        assert_eq!(wire(&instance_load_spawn_point(1, 0.0, 0.0, 0)), 26);
        assert_eq!(wire(&instance_loaded(0xBAAD_F00D)), 6);
        assert_eq!(wire(&instance_load_finish()), 2);
        assert_eq!(wire(&instance_manifest_phase(1)), 3);
        assert_eq!(wire(&instance_manifest_done(2, 0xFFFF)), 9);
        assert_eq!(wire(&item_stream_create(1, 0)), 5);
        assert_eq!(wire(&item_set_active_weapon_set(1, 0)), 5);
        assert_eq!(wire(&item_weapon_set(1, 0, 0, 0)), 13);
        assert_eq!(wire(&inventory_create_bag(1, 1, 0, 1, 20, 0)), 13);
        assert_eq!(wire(&update_gold_storage(1, 0)), 8);
        assert_eq!(wire(&faction_max(FACTION_MAX_KURZICK, 0)), 6);
        assert_eq!(wire(&hard_mode_unlocked(1)), 3);
        assert_eq!(wire(&map_update_current(146)), 5);
        assert_eq!(wire(&ready_for_map_spawn()), 6);
        assert_eq!(wire(&pvp_item_stream_end()), 2);
        assert_eq!(wire(&account_feature(1, 10, 0)), 8);
        assert_eq!(wire(&agent_update_attribute_points(1, 50, 50)), 8);
        assert_eq!(wire(&player_update_profession(1, 0, 0, 0)), 9);
        assert_eq!(wire(&player_update_unlocked_professions(1, 0)), 10);
        assert_eq!(wire(&world_update_load_time(0)), 6);
        assert_eq!(
            wire(&world_create_agent(
                1,
                0x3000_0001,
                (0.0, 0.0),
                0,
                (1.0, 0.0),
                288.0,
                1
            )),
            99
        );
        assert_eq!(
            wire(&agent_property_update_int(AGENT_PROPERTY_ENERGY, 1, 25)),
            14
        );
        assert_eq!(
            wire(&agent_property_update_float(
                AGENT_PROPERTY_ENERGY_REGEN,
                1,
                0.033
            )),
            14
        );
        assert_eq!(wire(&agent_initial_effects(1, 0)), 10);
        assert_eq!(wire(&update_agent_visual_equipment(1, [0; 9])), 42);
        assert_eq!(wire(&agent_display_cape(1, 1)), 7);
        assert_eq!(wire(&world_update_controlled_agent(1, 3)), 10);
        assert_eq!(wire(&party_create(1)), 4);
        assert_eq!(wire(&party_player_add(1, 1, 1)), 7);
        assert_eq!(wire(&party_member_stream_end(1)), 4);
        assert_eq!(wire(&character_update_party(1, 1)), 5);
        assert_eq!(wire(&world_simulation_tick(16)), 6);
        assert_eq!(wire(&ping_request()), 2);
        assert_eq!(wire(&ping_reply(20)), 6);
    }

    #[test]
    fn the_counted_messages_are_exactly_as_long_as_their_counts_say() {
        assert_eq!(
            wire(&instance_load_player_name("")),
            4,
            "an empty name is 2 + 2"
        );
        assert_eq!(
            wire(&instance_load_player_name("Asura")),
            14,
            "5 units: 2 + 2 + 10"
        );
        assert_eq!(
            wire(&character_update_info("")),
            28,
            "the reference's empty name"
        );
        assert_eq!(wire(&character_update_info("Asura")), 38);
        assert_eq!(wire(&player_create(1, 2, 0, "Asura")), 35, "25 + 2 + 10");
        assert_eq!(wire(&pvp_update_unlocked_skills(&[])), 4);
        assert_eq!(wire(&pvp_update_unlocked_skills(&[1, 2, 3])), 16);
        assert_eq!(wire(&pvp_update_unlocked_heroes(&[0xFFFF_FFFF; 8])), 36);
        assert_eq!(wire(&update_unlocked_skills(&[])), 4);
        assert_eq!(wire(&agent_update_attributes(1, &[0; 42])), 176);
        assert_eq!(wire(&skillbar_update(1, &[0; 8], &[], 0)), 43);
        assert_eq!(
            wire(&skillbar_update(1, &[0; 8], &[0; 8], 1)),
            75,
            "the reference sends eight skills and eight PvP masks"
        );
        assert_eq!(
            wire(&map_update_unlocked_list([&[][..], &[], &[], &[], &[]])),
            12,
            "five empty lists are five counts"
        );
        assert_eq!(
            wire(&map_update_unlocked_list([&[7][..], &[8], &[], &[], &[9]])),
            24,
            "each list is a count plus its dwords"
        );
    }

    #[test]
    fn character_update_factions_is_fifteen_dwords_of_body() {
        let frame = character_update_factions(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        assert_eq!(wire(&frame), 62);
        assert_eq!(frame.body.len(), 60, "fifteen dwords");
    }

    #[test]
    fn the_header_is_the_servers_and_the_body_follows_it() {
        let frame = map_update_current(146);
        let bytes = frame.encode();
        assert_eq!(&bytes[0..2], &0x0099u16.to_le_bytes());
        assert_eq!(&bytes[2..4], &146u16.to_le_bytes());
        assert_eq!(bytes[4], 0);
    }
}

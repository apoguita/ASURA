//! The creation handlers: the four messages a client sends from the creation screen.
//!
//! The reference keeps these in `src/server/game/Handlers/CharacterHandler.cpp`. The creation flow and
//! its content - the professions, the starting equipment, the starting inventory - stay in
//! `game/mechanics/creation`, which publishes them; this file carries the four messages to it.

use super::*;
use crate::game::mechanics::creation::{
    creation_equipment_rows, creation_inventory, equipped_items, CREATION_ENERGY, CREATION_HEALTH,
};

/// ---- character creation (a connection in a `MapType_CharacterCreation` instance) -------------
///
/// The four messages `GameSrv_ProcessPlayerMessage` dispatches to `GameSrv_HandleCharCreation*`.
/// The block each answers with is the reference's own order; the equipment is memory, not rows,
/// until the confirm writes it (`GameSrv_CreatePlayerBags` runs there and nowhere else).
pub async fn handle_request_player(
    stream: &mut TcpStream,
    session: &mut Session,
    player: &mut Player,
    trace: bool,
) -> Result<bool, GameError> {
    let Some(creation) = player.creation() else {
        return Ok(true);
    };
    let agent_id = player.agent_id();
    let inventory = creation_inventory(creation);
    let mut frames = vec![builders::instance_player_data_start()];
    frames.push(builders::item_stream_create(ITEM_STREAM, 0));
    frames.extend(inventory_block(&inventory));
    frames.push(builders::item_set_active_weapon_set(ITEM_STREAM, 0));
    for slot in 0..4u8 {
        frames.push(builders::item_weapon_set(ITEM_STREAM, slot, 0, 0));
    }
    frames.push(builders::update_gold_storage(ITEM_STREAM, 0));
    for faction in [
        builders::FACTION_MAX_KURZICK,
        builders::FACTION_MAX_LUXON,
        builders::FACTION_MAX_BALTHAZAR,
        builders::FACTION_MAX_IMPERIAL,
    ] {
        frames.push(builders::faction_max(faction, 10_000));
    }
    frames.push(builders::agent_update_attribute_points(agent_id, 50, 50));
    frames.push(builders::player_update_profession(
        agent_id,
        creation.profession,
        0,
        0,
    ));
    frames.push(builders::player_update_unlocked_professions(
        agent_id,
        creation_screen_unlocked_professions(),
    ));
    frames.push(builders::skillbar_update(agent_id, &[0; 8], &[0; 8], 1));
    frames.push(builders::update_unlocked_skills(&[]));
    frames.push(builders::character_update_factions(
        &builders::CharacterProgress::default(),
    ));
    frames.push(builders::agent_update_attributes(agent_id, &[0; 42]));
    // `GameSrv_SendPlayerHealthEnergy` (`GmPlayer.c:178-199`): three **int** properties with the
    // creation path's own numbers -- energy 20, health 100, property 64 zero. The load block's numbers
    // differ (25, 400, a regen float and a level), because the reference sends different ones there.
    frames.push(builders::agent_property_update_int(
        builders::AGENT_PROPERTY_ENERGY,
        agent_id,
        CREATION_ENERGY,
    ));
    frames.push(builders::agent_property_update_int(
        builders::AGENT_PROPERTY_HEALTH,
        agent_id,
        CREATION_HEALTH,
    ));
    frames.push(builders::agent_property_update_int(
        AGENT_PROPERTY_VALUE64,
        agent_id,
        0,
    ));
    frames.push(builders::instance_player_data_done());
    send(stream, session, frames, trace).await?;
    Ok(true)
}

/// `GameSrv_HandleCharCreationRequestArmors`: the PvP half. This account has no unlocked skills,
/// no heroes and no PvP items, which is what an empty stream says.
pub async fn handle_request_armors(
    stream: &mut TcpStream,
    session: &mut Session,
    trace: bool,
) -> Result<bool, GameError> {
    let frames = vec![
        builders::pvp_update_unlocked_skills(&[]),
        builders::pvp_update_unlocked_heroes(&[]),
        builders::pvp_item_stream_end(),
    ];
    send(stream, session, frames, trace).await?;
    Ok(true)
}

/// `GameSrv_HandleCharCreationChangeProf` (`0x8060`: campaign, profession): swap the equipped set
/// and tell the client, which is the whole of what the reference does here.
pub async fn handle_change_profession(
    stream: &mut TcpStream,
    session: &mut Session,
    frame: &Frame,
    player: &mut Player,
    trace: bool,
) -> Result<bool, GameError> {
    let Some(mut creation) = player.creation() else {
        return Ok(true);
    };
    if frame.body().len() < 2 {
        return Err(GameError::Framing(FrameError::Incomplete { needed: 4 }));
    }
    let previous = creation;
    creation.campaign = frame.body()[0];
    creation.profession = frame.body()[1];
    player.set_creation(Some(creation));

    let agent_id = player.agent_id();
    let inventory = creation_inventory(creation);
    let mut frames = Vec::new();
    // The client still holds the set the previous change put there, and it asserts if an item is
    // created into a slot it thinks is occupied - `Assertion: !bagParent->GetItem(slot)`,
    // `ItCliInv.cpp(105)`, build 39112, measured 2026-10-28 as the crash the moment "select campaign"
    // opened. The reference takes the old pieces off the client with `ITEM_REMOVE` before it sends the
    // new ones (`0x014D x5` in
    // `recordings/client/feature/character_creation/character_creation-live-necro_factions.jsonl`,
    // right after the profession moved); this server renumbers its item ids per set, so it clears the
    // equipped bag on every change.
    for item in equipped_items(previous) {
        frames.push(builders::item_remove(ITEM_STREAM, item.id as u32));
    }
    frames.push(builders::update_unlocked_skills(&[]));
    frames.push(builders::player_update_profession(
        agent_id,
        creation.profession,
        0,
        0,
    ));
    // `GameSrv_SendBagItems` for the equipped bag: each item, then where it sits.
    let equipped_bag = inventory
        .bags
        .iter()
        .find(|bag| bag.bag_type == 2)
        .map(|bag| bag.bag_id);
    for item in inventory
        .items
        .iter()
        .filter(|item| item.bag_id == equipped_bag)
    {
        // The **named** creator: live sends `CREATE_NAMED_ITEM` on this path, 180-184 bytes with the
        // item's name and its modifiers (the five `0x0161` after each `0x0060` in the capture above).
        // The unnamed form leaves the client an item it holds without a name, and the creation screen
        // asserts `ptr->IsDetailHigh()` (`ItCliApi.cpp(698)`) when it asks for the details.
        frames.push(builders::create_named_item(&named_item(item)));
        if let (Some(bag), Some(slot)) = (item.bag_id, item.slot) {
            frames.push(builders::item_move_to_location(
                ITEM_STREAM,
                item.id as u32,
                bag as u16,
                slot as u8,
            ));
        }
    }
    send(stream, session, frames, trace).await?;
    Ok(true)
}

/// `GameSrv_HandleCharCreationConfirm` (`0x808B`: name, appearance, flags): write the rows and say
/// so. The reference answers `CHAR_CREATION_ERROR` when the name is taken; this server has the same
/// unique-per-account rule to enforce, and answers the same way.
pub async fn handle_confirm(
    stream: &mut TcpStream,
    session: &mut Session,
    frame: &Frame,
    world: &World,
    player: &mut Player,
    trace: bool,
) -> Result<bool, GameError> {
    let Some(creation) = player.creation() else {
        return Ok(true);
    };
    let body = &frame.body();
    if body.len() < 2 {
        return Err(GameError::Framing(FrameError::Incomplete { needed: 4 }));
    }
    let units = u16::from_le_bytes([body[0], body[1]]) as usize;
    let name = String::from_utf16_lossy(
        &(0..units.min(name_max_units()))
            .map(|index| {
                let at = 2 + index * 2;
                u16::from_le_bytes([
                    body.get(at).copied().unwrap_or(0),
                    body.get(at + 1).copied().unwrap_or(0),
                ])
            })
            .collect::<Vec<u16>>(),
    );
    if !asura_rules::name_is_acceptable(&name) {
        send(
            stream,
            session,
            vec![builders::char_creation_error(
                GM_ERROR_CHARACTER_NAME_ALREADY_EXIST,
            )],
            trace,
        )
        .await?;
        return Ok(true);
    }

    let equipped = creation_equipment_rows(creation);
    // The traits the confirm implies: the packed appearance the client sent (`msg->config`, four
    // bytes of `Appearance`), the profession the screen last chose, and the one unlocked-profession
    // bit the reference sets (`character.unlocked_professions = 1 << app.primary_profession`).
    let config_at = 2 + units * 2;
    let appearance = u32::from_le_bytes([
        body.get(config_at).copied().unwrap_or(0),
        body.get(config_at + 1).copied().unwrap_or(0),
        body.get(config_at + 2).copied().unwrap_or(0),
        body.get(config_at + 3).copied().unwrap_or(0),
    ]);
    // What a new character is worth is the pack's (`config/World/Vanilla/character_defaults.json`):
    // level, gold and secondary profession are values the world states. The unlocked-profession bitmap is
    // not among them - it is one bit for the profession the player chose, which the confirm derives.
    let defaults = match crate::character_defaults::current() {
        Ok(defaults) => defaults,
        Err(err) => {
            eprintln!("world: cannot create {name:?}: {err}");
            send(
                stream,
                session,
                vec![builders::char_creation_error(
                    GM_ERROR_CHARACTER_NAME_ALREADY_EXIST,
                )],
                trace,
            )
            .await?;
            return Ok(true);
        }
    };
    let traits = db_game::Traits {
        appearance,
        profession: creation.profession,
        unlocked_professions: 1 << creation.profession,
        gold: defaults.gold(),
        campaign: creation.campaign,
        secondary_profession: defaults.secondary_profession(),
        level: defaults.level(),
    };
    // Where the character starts, which is not where it was made. A live creation redirects a Factions
    // character to map 212 (`crates/asura-db/migrations/0017_starter_outpost.sql`). Every other
    // campaign is put in Ascalon City, which is the Prophecies starting outpost and is already in the
    // catalogue; the log says when that stand-in was used, so it is never mistaken for a measurement.
    // Where the character is spawned, by the expansion source the screen chose. It is the world pack's
    // file (`config/World/Vanilla/character_spawn.json`, `docs/reference/decisions/layer-decisions.md`, D23, D24), not a match in this file:
    // one map per expansion source. A source with no map at all (a PvP character) is refused rather
    // than guessed.
    let spawns = match crate::spawns::current() {
        Ok(spawns) => spawns,
        Err(err) => {
            eprintln!("world: cannot create {name:?}: {err}");
            send(
                stream,
                session,
                vec![builders::char_creation_error(
                    GM_ERROR_CHARACTER_NAME_ALREADY_EXIST,
                )],
                trace,
            )
            .await?;
            return Ok(true);
        }
    };
    let Some(placed) = spawns.for_campaign(creation.campaign) else {
        eprintln!(
            "world: campaign {} has no spawn map in the pack; refusing to create {name:?} rather than \
             putting it somewhere nobody has measured",
            creation.campaign
        );
        send(
            stream,
            session,
            vec![builders::char_creation_error(
                GM_ERROR_CHARACTER_NAME_ALREADY_EXIST,
            )],
            trace,
        )
        .await?;
        return Ok(true);
    };
    let outpost = placed.map_id;
    match db_inventory::create_character(
        &world.pool,
        player.account_id(),
        &name,
        outpost,
        traits,
        &equipped,
    )
    .await
    {
        Ok(character) => {
            eprintln!(
                "world: created character {name:?} ({} items, campaign {} profession {}) in map {}",
                equipped.len(),
                creation.campaign,
                creation.profession,
                outpost
            );
            send(
                stream,
                session,
                vec![
                    builders::char_creation_success(character.to_bytes_le(), &name, &[]),
                    // The client reconnects naming the map it believes it is in, and that belief came
                    // from the transfer, which named the creation instance: without this it dials back
                    // into the creation screen and waits there forever (measured in this server's own
                    // capture, 2026-10-28, and again after the first attempt with the wrong shape).
                    //
                    // The **login** shape, five bytes: `map_update_current_redirect` is the
                    // four-byte-header generation (a `u16` zero, then two dwords) and this client
                    // parses two-byte headers, so it ignored that one and kept dialling map 0
                    // (`docs/reference/client-protocol.md` #5e).
                    builders::map_update_current(outpost as u16),
                ],
                trace,
            )
            .await?;
        }
        Err(err) => {
            eprintln!("world: could not create {name:?}: {err}");
            send(
                stream,
                session,
                vec![builders::char_creation_error(
                    GM_ERROR_CHARACTER_NAME_ALREADY_EXIST,
                )],
                trace,
            )
            .await?;
        }
    }
    Ok(true)
}

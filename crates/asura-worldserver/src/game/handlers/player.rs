//! The player-block handler: the request that ends with the message taking a client off the loading
//! screen.
//!
//! The reference keeps this one in `src/server/game/Handlers/CharacterHandler.cpp` and
//! `MiscHandler.cpp`. The block is built by `game/entities/player`; this file carries the request in
//! and sends what it built.

use super::*;

/// The client asks who else is here. This is the big block, and its last message
/// (`INSTANCE_LOAD_FINISH`) is what takes the client off the loading screen.
///
/// Everyone already in the map goes after it, in the reference's own order for a joining client
/// (`GameSrv_SendWorldAgents`, `GmAgent.c:310-331`), and the same frames go out to *them* so they
/// learn about this one. The reference broadcasts the player create there and unicasts the rest;
/// the order is what matters here, and it is the same in both directions.
pub async fn handle_request_players(
    stream: &mut TcpStream,
    session: &mut Session,
    world: &World,
    player: &mut Player,
    trace: bool,
) -> Result<bool, GameError> {
    let replay = world.replay.as_ref();
    // The load block's own cost, measured. The first instance a fresh process serves is the one that warms
    // every cache behind it, so these numbers are what separates a server-side delay from a client-side
    // one: a block built in single-digit milliseconds cannot be why a client's load drags.
    let load_started = Instant::now();
    // The instance the character is standing in is reached, so it is unlocked - that is the whole rule
    // (`docs/reference/game-world-model.md`): the walk sets the bit, the bit never gates the walk. The creation
    // instance is not a place a character "reaches", so it sets nothing.
    let here = player.instance().map().map_id;
    // The last outpost this character stood in, kept for the base rule (docs/reference/game-world-model.md): a travel
    // into an explorable must not overwrite it, which is why only an outpost is written here.
    if player.instance().map().map_type == map_kinds().main_town {
        if let Err(err) = db_game::remember_outpost(&world.pool, player.character(), here).await {
            eprintln!("world: could not remember the outpost {here}: {err}");
        }
    }
    if player.instance().map().map_type != map_kinds().character_creation {
        match db_game::unlock_instance(&world.pool, player.character(), here).await {
            Ok(true) => eprintln!(
                "world: {:?} unlocked instance {here}: first arrival",
                player.name()
            ),
            Ok(false) => {}
            Err(err) => eprintln!("world: could not record the arrival in instance {here}: {err}"),
        }
    }
    // The bitmaps the client is given: the character's own reached instances, and the one it is in even
    // if the database could not be read - a client told it is nowhere can offer nowhere.
    let unlocked = match db_game::unlocked_instances(&world.pool, player.character()).await {
        Ok(instances) => unlock_bitmap(&instances),
        Err(err) => {
            eprintln!("world: could not read the reached instances, sending this one only: {err}");
            unlock_bitmap(&[here])
        }
    };
    let unlocked_at = load_started.elapsed();
    let block_started = Instant::now();
    let mut block = match replay {
        Some(replay) if replay.pick.wants(crate::replay::BlockName::Players) => {
            replay.players.clone()
        }
        _ => player_block(&crate::player::PlayerBlock {
            name: player.name(),
            standing: player.standing(),
            inventory: player.inventory(),
            started: player.started(),
            player_id: player.player_id(),
            agent_id: player.agent_id(),
            traits: player.traits(),
            unlocked: &unlocked,
            agent: &world.agent,
        }),
    };
    let others = player.instance().others(player.player_id());
    eprintln!(
        "world: [+{} ms] load block for instance {here}: {} frame(s); reached list {} ms, block {} ms, total {} ms",
        crate::stamp_ms(),
        block.len(),
        unlocked_at.as_millis(),
        block_started.elapsed().as_millis(),
        load_started.elapsed().as_millis()
    );
    for peer in &others {
        block.extend(introduction(peer, &world.agent));
    }
    let me = PeerView {
        player_id: player.player_id(),
        agent_id: player.agent_id(),
        name: player.name().to_string(),
        standing: player.standing(),
        equipped: player.inventory().equipped_item_ids(),
    };
    if !others.is_empty() {
        // The introduction, then what the others need in order to draw this player:
        // `GameSrv_BroadcastPlayerEquippedItems` (`GmItem.c:129-160`, the *unnamed* item form),
        // then `GameSrv_BroadcastUpdateAgentVisualEquipment` and `GameSrv_BroadcastAgentDisplayCape`
        // - the reference's own order in the same handler (`GameSrv.c:1643-1650`).
        let mut mine = introduction(&me, &world.agent);
        let equipped_bag = player
            .inventory()
            .bags
            .iter()
            .find(|bag| bag.bag_type == 2)
            .map(|bag| bag.bag_id);
        for item in player
            .inventory()
            .items
            .iter()
            .filter(|item| item.bag_id == equipped_bag)
        {
            mine.push(builders::create_unnamed_item(&named_item(item)));
        }
        mine.push(equipment_frame(player));
        mine.push(builders::agent_display_cape(player.agent_id(), 1));
        player.instance().broadcast(player.player_id(), &mine);
    }
    send(stream, session, block, trace).await?;
    // The load is over: the block's last message was `INSTANCE_LOAD_FINISH`. Only now may the instance's
    // clock tick this player - measured, not assumed: 36 live loads carry no `0x001E` between the client's
    // spawn request and this one, and this world used to send 49 into that window from the unasked block
    // onwards (see `run`'s comment in connection.rs).
    player.instance().loaded(player.player_id());
    Ok(true)
}

//! The item handlers: the five messages a client sends about what it carries.
//!
//! Moved out of `game/entities/item` in cut 4d, beside the agent handlers, for the same reason: the
//! noun holds the rows and the rules, the handler holds the conversation.

use super::*;
pub async fn handle_request_items(
    stream: &mut TcpStream,
    session: &mut Session,
    world: &World,
    player: &mut Player,
    trace: bool,
) -> Result<bool, GameError> {
    let replay = world.replay.as_ref();
    let block = match replay {
        Some(replay) if replay.pick.wants(crate::replay::BlockName::Items) => replay.items.clone(),
        _ => items_block(
            player.instance().map(),
            &player.inventory(),
            player.traits().gold,
        ),
    };
    send(stream, session, block, trace).await?;
    Ok(true)
}

/// The client drops an item (`GameSrv_HandleDropItem`, `0x802C`: `{item_id: u32, count: u8}`).
///
/// **This is where this port leaves the reference behind, deliberately.** The reference's handler
/// creates an agent for the item and returns: it never removes the item from its bag, never
/// frees its id, never sends `ITEM_REMOVE`, and bails out entirely when the count is smaller than
/// the stack (`TODO: we must still implement this`) -- so dropping a stacked item there does
/// nothing at all, and the client always sends `count = 1` for a stack. Its item agent also
/// carries the item's *id* as its model id, so the thing on the ground is drawn as nothing. A
/// measured live session (`docs/reference/inventory-model.md`) showed all three in one log.
///
/// What this does instead: take `count` off the stack for real, write it down, and say so in the
/// log. It sends nothing -- both messages it could send (`ITEM_REMOVE` and an item agent) crashed a
/// live client, and neither is in any capture. `drop_messages` carries the whole argument.
pub async fn handle_drop_item(
    frame: &Frame,
    world: &World,
    player: &mut Player,
) -> Result<bool, GameError> {
    let body = &frame.body();
    if body.len() < 5 {
        return Err(GameError::Framing(FrameError::Incomplete { needed: 7 }));
    }
    let item_id = i32::from_le_bytes([body[0], body[1], body[2], body[3]]);
    let count = i32::from(body[4]);
    drop_item(world, player, item_id, count).await?;
    Ok(true)
}

/// The client moves an item to a slot (`GAME_CMSG_ITEM_MOVE`, `0x8072`: `{item_id: u32,
/// bag_id: u16, slot: u8}` -- the exact inverse of the `ITEM_MOVE_TO_LOCATION` we send). Measured
/// from a live client: dragging an item produced two of these and nothing else.
///
/// The client has already moved the item in its own model by the time it asks, so a refusal is
/// answered by telling it where the item really is rather than by silence.
pub async fn handle_item_move(
    stream: &mut TcpStream,
    session: &mut Session,
    frame: &Frame,
    world: &World,
    player: &mut Player,
    trace: bool,
) -> Result<bool, GameError> {
    let body = &frame.body();
    if body.len() < 7 {
        return Err(GameError::Framing(FrameError::Incomplete { needed: 9 }));
    }
    let item_id = i32::from_le_bytes([body[0], body[1], body[2], body[3]]);
    let bag_id = i32::from(u16::from_le_bytes([body[4], body[5]]));
    let slot = i32::from(body[6]);
    move_item(
        stream,
        session,
        world,
        player,
        Placement {
            item_id,
            bag_id,
            slot,
        },
        trace,
    )
    .await?;
    Ok(true)
}

/// The client equips an item (`GAME_CMSG_EQUIP_ITEM`, `0x8030`: `{item_id: u32}`).
///
/// The reference has no handler for this at all, so the behaviour here is this project's: the
/// item moves into the equipped-items bag slot its type belongs to, whatever was worn there goes
/// into the first free carrying slot, and the client is told what it is now wearing. The
/// type-to-slot table is `builders::equipped_slot`.
pub async fn handle_equip_item(
    stream: &mut TcpStream,
    session: &mut Session,
    frame: &Frame,
    world: &World,
    player: &mut Player,
    trace: bool,
) -> Result<bool, GameError> {
    let body = &frame.body();
    if body.len() < 4 {
        return Err(GameError::Framing(FrameError::Incomplete { needed: 6 }));
    }
    let item_id = i32::from_le_bytes([body[0], body[1], body[2], body[3]]);
    equip_item(stream, session, world, player, item_id, trace).await?;
    Ok(true)
}

/// `GAME_CMSG_UNEQUIP_ITEM` (`0x804F`, 6 bytes: `{u8, u16, u8}`). Its layout carries no item id,
/// and nothing here has seen a live one, so this does not guess: unequipping by dragging an item
/// out of the equipped-items bag arrives as `ITEM_MOVE` above and is handled there.
pub fn handle_unequip_item() -> Result<bool, GameError> {
    eprintln!(
        "world: 0x804F (unequip) is not implemented: it carries no item id and no live capture \
         has shown what it means; dragging the item out of the equipped bag works instead"
    );
    Ok(true)
}

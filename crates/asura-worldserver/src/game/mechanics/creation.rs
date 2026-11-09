//! Character creation: the instance and the choices a new character is made of.
//!
//! The reference's `MapType_CharacterCreation` - the only map type that is a conversation
//! rather than a place (`docs/work/session-handoff.md`, M5). [`Creation`] is what a creation connection has
//! chosen so far; the two blocks are the reference's own default equipment and inventory for
//! that choice (`GetDefaultEquipments`, generated into `default_equipment.rs`).

use crate::*;

/// The energy and health a character-creation screen's player-data block reports. **Not** the load block's
/// numbers: `GameSrv_SendPlayerHealthEnergy` (`GmPlayer.c:178-199`) sends 20 and 100, while the load path
/// sends 25 and 400 (measured from the reference's own send log). Two paths, two sets of numbers, and mixing
/// them -- which this server did until the log was read -- is a mismatched block.
pub const CREATION_ENERGY: u32 = 20;

/// See [`CREATION_ENERGY`].
pub const CREATION_HEALTH: u32 = 100;

/// `GM_ERROR_CHARACTER_NAME_ALREADY_EXIST` (`GmError.h`): the code the reference answers a taken name with
/// (`GameSrv.c:1939`).
pub const GM_ERROR_CHARACTER_NAME_ALREADY_EXIST: u32 = 0x0106;

/// What a character-creation connection has chosen so far.
///
/// The reference keeps exactly this on its player (`char_creation_campaign`, `primary_profession`) and
/// nothing else: the equipment is not stored either - it is re-derived from the generated table every time
/// the profession changes (`GetDefaultEquipments`) - and only becomes rows at confirm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Creation {
    pub campaign: u8,
    pub profession: u8,
}

impl Creation {
    /// The reference's starting state: `HandleCharCreationRequestPlayer` sets Warrior, and nothing sets the
    /// campaign until the client says so - so a zeroed `Campaign` (`None`, the PvP table) is what a fresh
    /// creation screen is built from.
    pub fn start() -> Creation {
        Creation {
            campaign: 0,
            profession: DEFAULT_PRIMARY_PROFESSION,
        }
    }
}

/// The bags and items a creation screen shows: the reference's own default set, with the equipped bag
/// holding the chosen profession's starting equipment.
///
/// Built in memory because that is where the reference keeps it too - `HandleCharCreationChangeProf`
/// frees and re-adds items in the player's `GmBag`s, and `HandleCharCreationConfirm` is the only place they
/// become database rows.
pub fn creation_inventory(creation: Creation) -> Inventory {
    use std::collections::HashMap;

    let equipped_bag = db_inventory::seed::BAGS
        .iter()
        .find(|bag| bag.bag_type == 2)
        .map(|bag| bag.bag_id);

    let mut items: Vec<db_inventory::Item> = Vec::new();
    // The capture's item ids mean nothing once rows are renumbered, so the mapping is kept explicitly:
    // a bag names its own item by seed id, and only this map knows which row that became.
    let mut by_seed: HashMap<i32, i32> = HashMap::new();
    let mut next_id = 1i32;

    // Everything the reference's default inventory holds outside the equipped bag -- the backpack's own
    // item and the belt pouch's -- so what the creation screen sees is what the confirm writes.
    for seed_item in db_inventory::seed::ITEMS
        .iter()
        .filter(|item| item.bag_id != equipped_bag)
    {
        by_seed.insert(seed_item.id, next_id);
        items.push(db_inventory::Item {
            id: next_id,
            bag_id: seed_item.bag_id,
            slot: seed_item.slot,
            file_id: seed_item.file_id,
            item_type: seed_item.item_type,
            model_id: seed_item.model_id,
            dye_tint: seed_item.dye_tint,
            dye_colors: seed_item.dye_colors,
            materials: seed_item.materials,
            unk1: seed_item.unk1,
            flags: seed_item.flags,
            value: seed_item.value,
            quantity: seed_item.quantity,
            profession: seed_item.profession,
            name: seed_item.name.to_vec(),
            modifiers: seed_item.modifiers.to_vec(),
        });
        next_id += 1;
    }

    for spec in crate::default_equipment::default_equipment(creation.campaign, creation.profession)
    {
        // `GetBagSlotForItemType`'s mapping, which is what `equip_item` uses too.
        let slot = builders::equipped_slot(spec.item_type).map(i32::from);
        items.push(db_inventory::Item {
            id: next_id,
            bag_id: equipped_bag,
            slot,
            file_id: i64::from(spec.file_id),
            item_type: i32::from(spec.item_type),
            model_id: spec.model_id as i32,
            dye_tint: i32::from(spec.dye_tint),
            dye_colors: i32::from(spec.dye_colors),
            materials: i32::from(spec.materials),
            unk1: i32::from(spec.unk1),
            flags: i64::from(spec.flags),
            value: spec.value as i32,
            quantity: spec.quantity as i32,
            profession: i32::from(spec.profession),
            name: spec.name.iter().map(|unit| i32::from(*unit)).collect(),
            modifiers: spec.modifiers.iter().map(|word| i64::from(*word)).collect(),
        });
        next_id += 1;
    }

    let bags = db_inventory::seed::BAGS
        .iter()
        .map(|seed_bag| db_inventory::Bag {
            bag_id: seed_bag.bag_id,
            bag_model_id: seed_bag.bag_model_id,
            bag_type: seed_bag.bag_type,
            slot_count: seed_bag.slot_count,
            assoc_item_id: by_seed.get(&seed_bag.assoc_item).copied().unwrap_or(0),
        })
        .collect();

    Inventory { bags, items }
}

/// The equipped bag's items for a choice: what the creation screen shows, and therefore what has to be
/// taken off the client before another set is sent into the same slots.
pub fn equipped_items(creation: Creation) -> Vec<db_inventory::Item> {
    let inventory = creation_inventory(creation);
    let equipped_bag = inventory
        .bags
        .iter()
        .find(|bag| bag.bag_type == 2)
        .map(|bag| bag.bag_id);
    inventory
        .items
        .into_iter()
        .filter(|item| item.bag_id == equipped_bag)
        .collect()
}

/// The equipped set as rows the confirm writes, owning their slices.
pub fn creation_equipment_rows(creation: Creation) -> Vec<db_inventory::NewItem> {
    let equipped_bag = db_inventory::seed::BAGS
        .iter()
        .find(|bag| bag.bag_type == 2)
        .map(|bag| bag.bag_id);
    crate::default_equipment::default_equipment(creation.campaign, creation.profession)
        .iter()
        .map(|spec| db_inventory::NewItem {
            bag_id: equipped_bag,
            slot: builders::equipped_slot(spec.item_type).map(i32::from),
            file_id: i64::from(spec.file_id),
            item_type: i32::from(spec.item_type),
            model_id: spec.model_id as i32,
            dye_tint: i32::from(spec.dye_tint),
            dye_colors: i32::from(spec.dye_colors),
            materials: i32::from(spec.materials),
            unk1: i32::from(spec.unk1),
            flags: i64::from(spec.flags),
            value: spec.value as i32,
            quantity: spec.quantity as i32,
            profession: i32::from(spec.profession),
            name: spec.name.iter().map(|unit| i32::from(*unit)).collect(),
            modifiers: spec.modifiers.iter().map(|word| i64::from(*word)).collect(),
        })
        .collect()
}

/// Re-read the inventory after a change, so the next message built from it is the new truth.
pub async fn reload(world: &World, player: &mut Player) -> Result<(), GameError> {
    let inventory = db_inventory::load(&world.pool, player.character())
        .await
        .map_err(GameError::Database)?;
    player.set_inventory(inventory);
    Ok(())
}

//! M4's world half: a client that was sent to this world connects, is believed, and gets a map.
//!
//! What is proved here, in the order the assertions appear:
//!
//! * a client presenting the token pair the auth service issued is accepted, and one presenting a
//!   token nobody issued is **closed with no message** (the reference's own behaviour);
//! * the four messages of the load block arrive in the reference's order and the character's name is in
//!   the third of them;
//! * each of the three things a client asks for is answered, and the item request's answer ends with
//!   the download manifest, whose last message names the map;
//! * the spawn point tells the client the map blob and where to stand â€” the two numbers without which
//!   the loading screen never ends;
//! * the players request ends with `INSTANCE_LOAD_FINISH`, which is what takes a client off the loading
//!   screen;
//! * the world keeps the clock running without being asked, and answers a ping;
//! * a dropped item becomes an agent of its own, and — as in the reference — stays in its bag;
//! * a character that leaves standing somewhere comes back standing there, and the slot it took goes
//!   back to the pool both times.
//!
//! With no database configured, each test prints a skip line and returns.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use asura_db::inventory as db_inventory;
use asura_db::login as db;
use asura_db::{placement, PgPool, Uuid};
use asura_protocol::client::game::GameVersion;
use asura_protocol::client::handshake;
use asura_protocol::client::peer::ReferenceClient;
use asura_protocol::client::Frame;
use asura_protocol::game;
use asura_world::{serve, MapEntry, World};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const TIMEOUT: Duration = Duration::from_secs(10);

fn skip() -> bool {
    if asura_db::config::database_url().is_none() {
        println!("ASURA_DATABASE_URL not set; skipping the end-to-end world test");
        return true;
    }
    false
}

fn unique_map() -> i32 {
    50_000 + (Uuid::new_v4().as_u128() % 10_000) as i32
}

fn unique_world() -> i32 {
    20_000 + (Uuid::new_v4().as_u128() % 10_000) as i32
}

/// A world server on a loopback port, and everything the test needs to check behind it.
struct Harness {
    pool: PgPool,
    world_id: i32,
    map_id: i32,
    address: SocketAddr,
    session: Uuid,
    account: Uuid,
    character: Uuid,
    /// The token the auth service "issued" for this harness's travel. Random per harness because the
    /// transfer table is keyed by it and these tests run in parallel.
    player_token: u32,
    server: tokio::task::JoinHandle<()>,
}

impl Harness {
    /// Start a world serving one map, and place a player in it the way the auth service would.
    async fn start() -> Option<Harness> {
        let url = asura_db::config::database_url()?;
        let pool = placement::connect(&url, 4)
            .await
            .expect("connect the database");

        let world_id = unique_world();
        let map_id = unique_map();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind a loopback port");
        let address = listener.local_addr().expect("local address");

        let world = Arc::new(World {
            pool: pool.clone(),
            world_id,
            maps: vec![MapEntry {
                map_id,
                // The reference's blob for the pre-Searing region.
                map_file_id: 0x1B97D,
                map_type: 0,
                spawn: (6602.0, 4485.0),
                spawn_plane: 0,
            }],
            replay: None,
        });
        let server = tokio::spawn(async move {
            let _ = serve(listener, world, true).await;
        });

        // The identity the login path would have produced.
        let seeded = db::seed_identity(
            &pool,
            Uuid::new_v4(),
            Uuid::new_v4(),
            &format!("m4-world-{}", Uuid::new_v4()),
            "Asura Test",
            map_id,
        )
        .await
        .expect("seed the identity");

        // What the auth service does for a travel: take the slot, then write the pending transfer.
        placement::register_world(
            &pool,
            world_id,
            "e2e-world",
            &address.ip().to_string(),
            i32::from(address.port()),
            "m4-test",
            &[(map_id, 2)],
        )
        .await
        .expect("register the world");
        placement::reserve(&pool, world_id, map_id, seeded.session)
            .await
            .expect("reserve");
        let player_token = (Uuid::new_v4().as_u128() & 0xFFFF_FFFF) as u32;
        placement::begin_transfer(
            &pool,
            player_token,
            world_id,
            map_id,
            seeded.session,
            seeded.account,
            seeded.character,
        )
        .await
        .expect("write the pending transfer");

        Some(Harness {
            pool,
            world_id,
            map_id,
            address,
            session: seeded.session,
            account: seeded.account,
            character: seeded.character,
            player_token,
            server,
        })
    }

    fn stop(self) {
        self.server.abort();
    }

    /// The version message a client presents for `player_token`.
    fn version(&self, player_token: u32) -> GameVersion {
        GameVersion {
            version: 39112,
            h0008: 1,
            map_token: self.world_id as u32,
            map_id: self.map_id as u32,
            player_token,
            account: self.account.to_bytes_le(),
            character: self.character.to_bytes_le(),
            h0038: 0,
            h003c: 0,
        }
    }

    /// Do what the auth service does for one travel: take a slot, then write the pending transfer.
    ///
    /// A second call is how a test gets a second session for the same character — the first transfer is
    /// claimed by the first connection and cannot be presented twice.
    async fn transfer(&self) -> u32 {
        placement::reserve(&self.pool, self.world_id, self.map_id, self.session)
            .await
            .expect("reserve");
        let player_token = (Uuid::new_v4().as_u128() & 0xFFFF_FFFF) as u32;
        placement::begin_transfer(
            &self.pool,
            player_token,
            self.world_id,
            self.map_id,
            self.session,
            self.account,
            self.character,
        )
        .await
        .expect("write the pending transfer");
        player_token
    }

    /// Wait for the slot a travel took to go back to the pool, which the world does when the client
    /// leaves. Returns whether it happened.
    async fn slot_released(&self) -> bool {
        for _ in 0..60 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            if placement::pool_counts(&self.pool, self.world_id, self.map_id)
                .await
                .expect("read the counter")
                == Some((2, 0))
            {
                return true;
            }
        }
        false
    }
}

/// A cleaner key exchange: send the game version, then the seed, then accept the reply.
async fn connect_properly(
    address: SocketAddr,
    version: &GameVersion,
) -> Option<(TcpStream, ReferenceClient)> {
    let mut stream = tokio::time::timeout(TIMEOUT, TcpStream::connect(address))
        .await
        .expect("connect before the timeout")
        .expect("connect");
    stream
        .write_all(&version.encode())
        .await
        .expect("write the game version");

    let mut client = ReferenceClient::with_seed(0x5EED5EED);
    stream
        .write_all(&client.client_seed_message())
        .await
        .expect("write the client seed");

    let mut seed = [0u8; handshake::SERVER_SEED_LEN];
    match tokio::time::timeout(TIMEOUT, stream.read_exact(&mut seed)).await {
        Ok(Ok(_)) => {}
        Ok(Err(_)) | Err(_) => return None,
    }
    client.accept_server_seed(&seed).expect("well-formed seed");
    Some((stream, client))
}

/// Read until a frame with `header` has arrived, and return everything read so far.
async fn read_until(
    stream: &mut TcpStream,
    client: &mut ReferenceClient,
    header: u16,
) -> Vec<Frame> {
    let mut frames: Vec<Frame> = Vec::new();
    let mut buffer = [0u8; 8192];
    while !frames.iter().any(|frame| frame.header == header) {
        let read = tokio::time::timeout(TIMEOUT, stream.read(&mut buffer))
            .await
            .expect("read before the timeout")
            .expect("read");
        assert!(read > 0, "the world closed the connection while answering");
        frames.extend(client.receive_game(&buffer[..read]).expect("game frames"));
    }
    frames
}

/// Send a client message: the header and an all-zero body of the reference's length.
async fn ask(stream: &mut TcpStream, client: &mut ReferenceClient, header: u16, size: usize) {
    let mut bytes = vec![0u8; size];
    bytes[0..2].copy_from_slice(&header.to_le_bytes());
    client.seal_raw(&mut bytes);
    stream.write_all(&bytes).await.expect("write the request");
}

#[tokio::test]
async fn a_placed_client_is_loaded_into_its_map() {
    if skip() {
        return;
    }
    let Some(harness) = Harness::start().await else {
        return;
    };

    let version = GameVersion {
        version: 39112,
        h0008: 1,
        map_token: harness.world_id as u32,
        map_id: harness.map_id as u32,
        player_token: harness.player_token,
        account: harness.account.to_bytes_le(),
        character: harness.character.to_bytes_le(),
        h0038: 0,
        h003c: 0,
    };
    let Some((mut stream, mut client)) = connect_properly(harness.address, &version).await else {
        panic!("the world refused a token the auth service had just issued");
    };

    // ---- the load block, which the world sends without being asked ---------------------------
    let frames = read_until(&mut stream, &mut client, game::INSTANCE_LOAD_INFO).await;
    let headers: Vec<u16> = frames.iter().map(|frame| frame.header).collect();
    assert_eq!(
        headers,
        vec![
            game::INSTANCE_LOAD_HEAD,
            game::INSTANCE_PLAYER_DATA_START,
            game::INSTANCE_LOAD_PLAYER_NAME,
            game::INSTANCE_LOAD_INFO,
        ],
        "the reference's four messages, in its order"
    );
    assert_eq!(frames[0].len(), 6, "INSTANCE_LOAD_HEAD");
    assert_eq!(frames[1].len(), 2, "INSTANCE_PLAYER_DATA_START");
    assert_eq!(frames[2].len(), 2 + 2 + 2 * 10, "a ten-unit name");
    assert_eq!(frames[3].len(), 16, "INSTANCE_LOAD_INFO");

    // The name the client is told is the character's own, in UTF-16 little endian.
    let name = &frames[2].body;
    let units = u16::from_le_bytes([name[0], name[1]]) as usize;
    assert_eq!(units, "Asura Test".encode_utf16().count());
    let decoded: String = (0..units)
        .map(|index| u16::from_le_bytes([name[2 + index * 2], name[3 + index * 2]]))
        .map(|unit| char::from_u32(u32::from(unit)).expect("a BMP character"))
        .collect();
    assert_eq!(decoded, "Asura Test");

    // The agent in the load info is the one the world will place.
    assert_eq!(
        u32::from_le_bytes([
            frames[3].body[0],
            frames[3].body[1],
            frames[3].body[2],
            frames[3].body[3]
        ]),
        asura_world::AGENT_ID
    );

    // ---- the item stream --------------------------------------------------------------------
    //
    // The world's clock keeps running while these arrive, so the block is checked for the reference's
    // *order* rather than for being the only thing on the wire.
    ask(&mut stream, &mut client, 0x8091, 4).await;
    let frames = read_until(&mut stream, &mut client, game::INSTANCE_MANIFEST_DONE).await;
    let headers: Vec<u16> = frames.iter().map(|frame| frame.header).collect();
    let at = |header: u16| {
        headers
            .iter()
            .position(|candidate| *candidate == header)
            .unwrap_or_else(|| panic!("{header:#06X} is missing from the item answer"))
    };
    assert!(
        at(game::ITEM_STREAM_CREATE) < at(game::MAP_UPDATE_CURRENT),
        "the item stream opens before the map is announced"
    );
    assert!(
        at(game::MAP_UPDATE_CURRENT) < at(game::READY_FOR_MAP_SPAWN),
        "the map is announced before the client is told it may spawn"
    );
    assert!(
        at(game::READY_FOR_MAP_SPAWN) < at(game::INSTANCE_MANIFEST_DONE),
        "and the manifest comes last"
    );
    assert_eq!(
        headers
            .iter()
            .filter(|h| **h == game::INSTANCE_MANIFEST_DONE)
            .count(),
        2,
        "the manifest sends two DONE messages"
    );
    let map_update = frames
        .iter()
        .find(|frame| frame.header == game::MAP_UPDATE_CURRENT)
        .expect("a map update");
    assert_eq!(
        map_update.len(),
        5,
        "the login shape, not the redirect's 12"
    );
    assert_eq!(
        u16::from_le_bytes([map_update.body[0], map_update.body[1]]),
        harness.map_id as u16
    );

    // ---- the spawn point --------------------------------------------------------------------
    ask(&mut stream, &mut client, 0x8088, 2).await;
    let frames = read_until(&mut stream, &mut client, game::INSTANCE_LOAD_SPAWN_POINT).await;
    let spawn = frames
        .iter()
        .find(|frame| frame.header == game::INSTANCE_LOAD_SPAWN_POINT)
        .expect("a spawn point");
    assert_eq!(spawn.len(), 26);
    assert_eq!(
        u32::from_le_bytes([spawn.body[0], spawn.body[1], spawn.body[2], spawn.body[3]]),
        0x1B97D,
        "the map blob the client must stream"
    );
    assert_eq!(
        f32::from_le_bytes([spawn.body[4], spawn.body[5], spawn.body[6], spawn.body[7]]),
        6602.0,
        "and where to stand, from the map's own config"
    );

    // ---- the players ------------------------------------------------------------------------
    ask(&mut stream, &mut client, 0x8090, 19).await;
    let frames = read_until(&mut stream, &mut client, game::INSTANCE_LOAD_FINISH).await;
    let headers: Vec<u16> = frames.iter().map(|frame| frame.header).collect();
    let position = |header: u16| headers.iter().position(|candidate| *candidate == header);
    assert!(
        position(game::INSTANCE_LOAD_FINISH).is_some(),
        "the load ends with the only message that leaves the loading screen"
    );
    assert!(
        position(game::INSTANCE_LOAD_FINISH) > position(game::WORLD_CREATE_AGENT),
        "and it comes after the agent exists"
    );
    for required in [
        game::PLAYER_CREATE,
        game::WORLD_CREATE_AGENT,
        game::WORLD_UPDATE_CONTROLLED_AGENT,
        game::PARTY_CREATE,
        game::CHARACTER_UPDATE_PARTY,
    ] {
        assert!(
            headers.contains(&required),
            "{required:#06X} is part of the load"
        );
    }
    let agent = frames
        .iter()
        .find(|frame| frame.header == game::WORLD_CREATE_AGENT)
        .expect("an agent");
    assert_eq!(agent.len(), 99, "the reference's fixed agent message");

    // ---- the clock, without being asked -----------------------------------------------------
    let frames = read_until(&mut stream, &mut client, game::WORLD_SIMULATION_TICK).await;
    assert!(
        frames
            .iter()
            .any(|frame| frame.header == game::WORLD_SIMULATION_TICK),
        "the world keeps time on its own"
    );

    // ---- the ping ---------------------------------------------------------------------------
    ask(&mut stream, &mut client, 0x800D, 2).await;
    let frames = read_until(&mut stream, &mut client, game::PING_REPLY).await;
    let reply = frames
        .iter()
        .find(|frame| frame.header == game::PING_REPLY)
        .expect("a ping reply");
    assert_eq!(
        u32::from_le_bytes([reply.body[0], reply.body[1], reply.body[2], reply.body[3]]),
        asura_world::PING_VALUE
    );

    // ---- leaving releases the slot ----------------------------------------------------------
    drop(stream);
    let mut released = false;
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let counts = placement::pool_counts(&harness.pool, harness.world_id, harness.map_id)
            .await
            .expect("read the counter");
        if counts == Some((2, 0)) {
            released = true;
            break;
        }
    }
    assert!(
        released,
        "the player's slot went back to the pool when it left"
    );

    let _ = harness.session;
    placement::remove_world(&harness.pool, harness.world_id)
        .await
        .expect("remove the world");
    harness.stop();
}

#[tokio::test]
async fn a_token_nobody_issued_is_closed_without_a_word() {
    if skip() {
        return;
    }
    let Some(harness) = Harness::start().await else {
        return;
    };

    let version = GameVersion {
        version: 39112,
        h0008: 1,
        map_token: harness.world_id as u32,
        map_id: harness.map_id as u32,
        player_token: harness.player_token ^ 0xFFFF, // never written to the transfer table
        account: harness.account.to_bytes_le(),
        character: harness.character.to_bytes_le(),
        h0038: 0,
        h003c: 0,
    };
    let accepted = connect_properly(harness.address, &version).await;
    assert!(
        accepted.is_none(),
        "a client with a token the auth service never issued must not reach the key exchange"
    );

    placement::remove_world(&harness.pool, harness.world_id)
        .await
        .expect("remove the world");
    harness.stop();
}

#[tokio::test]
async fn a_character_that_is_not_the_one_transferred_is_refused() {
    if skip() {
        return;
    }
    let Some(harness) = Harness::start().await else {
        return;
    };

    // A real token, but the wrong character: the pair is right and the identity is not.
    let version = GameVersion {
        version: 39112,
        h0008: 1,
        map_token: harness.world_id as u32,
        map_id: harness.map_id as u32,
        player_token: harness.player_token,
        account: harness.account.to_bytes_le(),
        character: Uuid::new_v4().to_bytes_le(),
        h0038: 0,
        h003c: 0,
    };
    assert!(
        connect_properly(harness.address, &version).await.is_none(),
        "the token names a character, and a different one is not accepted"
    );

    placement::remove_world(&harness.pool, harness.world_id)
        .await
        .expect("remove the world");
    harness.stop();
}

/// Send a client message with a body.
async fn say(stream: &mut TcpStream, client: &mut ReferenceClient, header: u16, body: &[u8]) {
    let mut bytes = Vec::with_capacity(2 + body.len());
    bytes.extend_from_slice(&header.to_le_bytes());
    bytes.extend_from_slice(body);
    client.seal_raw(&mut bytes);
    stream.write_all(&bytes).await.expect("write the message");
}

#[tokio::test]
async fn a_dropped_item_leaves_the_stack_and_lands_on_the_ground() {
    if skip() {
        return;
    }
    let Some(harness) = Harness::start().await else {
        return;
    };

    let version = harness.version(harness.player_token);
    let Some((mut stream, mut client)) = connect_properly(harness.address, &version).await else {
        panic!("the world refused a token the auth service had just issued");
    };
    // The load block, so the character exists on this connection before anything is asked of it.
    read_until(&mut stream, &mut client, game::INSTANCE_LOAD_INFO).await;

    // The world seeds the default inventory on the way in, so the cupcake's id is only known now.
    let inventory = db_inventory::load(&harness.pool, harness.character)
        .await
        .expect("read the inventory");
    assert_eq!(
        inventory.bags.len(),
        6,
        "a character that has never been played was given the reference's bags"
    );
    let cupcake = inventory.items_in(1)[0].clone();
    assert_eq!(cupcake.slot, Some(0));
    assert_eq!(cupcake.quantity, 100);
    assert_eq!(cupcake.model_id, 22269, "the cupcake's own model");

    // `GAME_CMSG_DROP_ITEM`: `{item_id: u32, count: u8}`. **One** of a stack of a hundred, which is what
    // a real client sends when an item is dragged out of the bag -- and what the reference refuses,
    // because its partial-stack branch is a `TODO`.
    let mut body = Vec::new();
    body.extend_from_slice(&(cupcake.id as u32).to_le_bytes());
    body.push(1);
    say(&mut stream, &mut client, 0x802C, &body).await;

    let frames = read_until(&mut stream, &mut client, game::WORLD_CREATE_AGENT).await;
    let headers: Vec<u16> = frames.iter().map(|frame| frame.header).collect();
    let agent_at = headers
        .iter()
        .position(|header| *header == game::WORLD_CREATE_AGENT)
        .expect("the drop created an agent");
    assert!(
        headers[..agent_at].contains(&game::ITEM_REMOVE),
        "the stack left the client's model before the new one appeared"
    );

    let agent = &frames[agent_at];
    assert_eq!(agent.len(), 99, "the reference's fixed agent message");
    assert_eq!(
        agent.body[8],
        game::AGENT_TYPE_ITEM,
        "an item agent, not a living one"
    );
    assert_eq!(
        u32::from_le_bytes([agent.body[4], agent.body[5], agent.body[6], agent.body[7]]),
        cupcake.model_id as u32,
        "and the agent draws the item's model -- not its id, which is what the reference sends"
    );

    // It was one of a hundred, so ninety-nine are left, in the slot they were in.
    let after = db_inventory::load(&harness.pool, harness.character)
        .await
        .expect("read the inventory");
    let left = after.item(cupcake.id).expect("the stack is still there");
    assert_eq!(left.quantity, 99, "one cupcake went to the ground");
    assert_eq!(left.slot, Some(0), "and the rest stayed where they were");

    // Dropping the rest empties the row: the item is gone, and nothing re-creates it.
    let mut body = (cupcake.id as u32).to_le_bytes().to_vec();
    body.push(99);
    say(&mut stream, &mut client, 0x802C, &body).await;
    ask(&mut stream, &mut client, 0x800D, 2).await;
    read_until(&mut stream, &mut client, game::PING_REPLY).await;
    let emptied = db_inventory::load(&harness.pool, harness.character)
        .await
        .expect("read the inventory");
    assert_eq!(
        emptied.item(cupcake.id),
        None,
        "the last of the stack went to the ground and the row with it"
    );

    // A drop of something that is not carried is answered with nothing at all, and does not end the
    // session: the reference's handler returns ERR_OK and sends no message.
    let mut body = 999_999u32.to_le_bytes().to_vec();
    body.push(1);
    say(&mut stream, &mut client, 0x802C, &body).await;
    ask(&mut stream, &mut client, 0x800D, 2).await;
    let frames = read_until(&mut stream, &mut client, game::PING_REPLY).await;
    assert!(
        !frames
            .iter()
            .any(|frame| frame.header == game::WORLD_CREATE_AGENT),
        "an item nobody carries creates no agent"
    );

    drop(stream);
    assert!(harness.slot_released().await, "the slot went back");
    placement::remove_world(&harness.pool, harness.world_id)
        .await
        .expect("remove the world");
    harness.stop();
}

#[tokio::test]
async fn an_item_can_be_moved_between_slots_and_the_server_agrees() {
    if skip() {
        return;
    }
    let Some(harness) = Harness::start().await else {
        return;
    };

    let version = harness.version(harness.player_token);
    let Some((mut stream, mut client)) = connect_properly(harness.address, &version).await else {
        panic!("the world refused a token the auth service had just issued");
    };
    read_until(&mut stream, &mut client, game::INSTANCE_LOAD_INFO).await;

    let inventory = db_inventory::load(&harness.pool, harness.character)
        .await
        .expect("read the inventory");
    let cupcake = inventory.items_in(1)[0].clone();
    assert_eq!(cupcake.slot, Some(0));

    // `GAME_CMSG_ITEM_MOVE`: `{item_id: u32, bag_id: u16, slot: u8}` -- what a live client sent when an
    // item was dragged, twice, and this server used to ignore.
    let move_to = |bag_id: u16, slot: u8, item: i32| {
        let mut body = (item as u32).to_le_bytes().to_vec();
        body.extend_from_slice(&bag_id.to_le_bytes());
        body.push(slot);
        body
    };
    say(&mut stream, &mut client, 0x8072, &move_to(1, 5, cupcake.id)).await;
    let frames = read_until(&mut stream, &mut client, game::ITEM_MOVE_TO_LOCATION).await;
    let echo = frames
        .iter()
        .rev()
        .find(|frame| frame.header == game::ITEM_MOVE_TO_LOCATION)
        .expect("the move is echoed");
    assert_eq!(
        u32::from_le_bytes([echo.body[2], echo.body[3], echo.body[4], echo.body[5]]),
        cupcake.id as u32
    );
    assert_eq!(
        u16::from_le_bytes([echo.body[6], echo.body[7]]),
        1,
        "the bag the client asked for"
    );
    assert_eq!(echo.body[8], 5, "and the slot");

    let moved = db_inventory::load(&harness.pool, harness.character)
        .await
        .expect("read the inventory");
    assert_eq!(
        moved.item(cupcake.id).and_then(|item| item.slot),
        Some(5),
        "and the row moved with it"
    );

    // Moving onto an occupied slot is refused, and the client is *told where the item really is* --
    // it has already moved it in its own model by the time it asks.
    let occupied = inventory
        .items_in(1)
        .into_iter()
        .find(|item| item.id != cupcake.id)
        .map(|item| item.slot.unwrap_or(1))
        .expect("a second cupcake in the backpack");
    say(
        &mut stream,
        &mut client,
        0x8072,
        &move_to(1, occupied as u8, cupcake.id),
    )
    .await;
    ask(&mut stream, &mut client, 0x800D, 2).await;
    let frames = read_until(&mut stream, &mut client, game::PING_REPLY).await;
    let snap_back = frames
        .iter()
        .rev()
        .find(|frame| frame.header == game::ITEM_MOVE_TO_LOCATION)
        .expect("a refusal puts the client's model back");
    assert_eq!(snap_back.body[8], 5, "still in the slot it really is in");
    let unchanged = db_inventory::load(&harness.pool, harness.character)
        .await
        .expect("read the inventory");
    assert_eq!(
        unchanged.item(cupcake.id).and_then(|item| item.slot),
        Some(5),
        "and nothing was written"
    );

    drop(stream);
    assert!(harness.slot_released().await, "the slot went back");
    placement::remove_world(&harness.pool, harness.world_id)
        .await
        .expect("remove the world");
    harness.stop();
}

#[tokio::test]
async fn an_item_can_be_taken_off_and_worn_again() {
    if skip() {
        return;
    }
    let Some(harness) = Harness::start().await else {
        return;
    };

    let version = harness.version(harness.player_token);
    let Some((mut stream, mut client)) = connect_properly(harness.address, &version).await else {
        panic!("the world refused a token the auth service had just issued");
    };
    // The players block is where the agent's equipment is sent, so read to the end of the load.
    read_until(&mut stream, &mut client, game::INSTANCE_LOAD_INFO).await;
    ask(&mut stream, &mut client, 0x8090, 19).await;
    let frames = read_until(&mut stream, &mut client, game::INSTANCE_LOAD_FINISH).await;

    // The ids are this character's own (the sequence's), not the capture's: the capture's 1..9 only
    // happen in a database where nothing has been seeded before.
    let inventory = db_inventory::load(&harness.pool, harness.character)
        .await
        .expect("read the inventory");
    let worn_ids = inventory.equipped_item_ids();
    let worn = frames
        .iter()
        .find(|frame| frame.header == game::UPDATE_AGENT_VISUAL_EQUIPMENT)
        .expect("the visual equipment");
    assert_eq!(
        u32::from_le_bytes([worn.body[4], worn.body[5], worn.body[6], worn.body[7]]),
        worn_ids[0],
        "the weapon slot holds the item the equipped bag says it does"
    );
    assert_ne!(worn_ids[0], 0, "and the character is dressed");

    let equipped_bag = inventory
        .bags
        .iter()
        .find(|bag| bag.bag_model_id == db_inventory::EQUIPPED_ITEMS_MODEL)
        .expect("the equipped-items bag")
        .bag_id;
    let weapon = inventory
        .items_in(equipped_bag)
        .into_iter()
        .find(|item| item.slot == Some(0))
        .cloned()
        .expect("the hammer in the weapon slot");

    // Take it off by dragging it out of the equipped bag: `ITEM_MOVE` into a free carrying slot.
    let free = db_inventory::first_free_carrying_slot(&harness.pool, harness.character)
        .await
        .expect("find a free slot")
        .expect("the backpack has room");
    let mut body = (weapon.id as u32).to_le_bytes().to_vec();
    body.extend_from_slice(&(free.0 as u16).to_le_bytes());
    body.push(free.1 as u8);
    say(&mut stream, &mut client, 0x8072, &body).await;
    let frames = read_until(
        &mut stream,
        &mut client,
        game::UPDATE_AGENT_VISUAL_EQUIPMENT,
    )
    .await;
    let now_wearing = frames
        .iter()
        .rev()
        .find(|frame| frame.header == game::UPDATE_AGENT_VISUAL_EQUIPMENT)
        .expect("taking the weapon off changes what the agent wears");
    assert_eq!(
        u32::from_le_bytes([
            now_wearing.body[4],
            now_wearing.body[5],
            now_wearing.body[6],
            now_wearing.body[7]
        ]),
        0,
        "the weapon slot is empty now"
    );

    // Put it back on: `GAME_CMSG_EQUIP_ITEM`, `{item_id: u32}`.
    say(
        &mut stream,
        &mut client,
        0x8030,
        &(weapon.id as u32).to_le_bytes(),
    )
    .await;
    let frames = read_until(
        &mut stream,
        &mut client,
        game::UPDATE_AGENT_VISUAL_EQUIPMENT,
    )
    .await;
    let wearing_again = frames
        .iter()
        .rev()
        .find(|frame| frame.header == game::UPDATE_AGENT_VISUAL_EQUIPMENT)
        .expect("equipping changes what the agent wears");
    assert_eq!(
        u32::from_le_bytes([
            wearing_again.body[4],
            wearing_again.body[5],
            wearing_again.body[6],
            wearing_again.body[7]
        ]),
        weapon.id as u32,
        "and the hammer is back in the weapon slot"
    );
    let after = db_inventory::load(&harness.pool, harness.character)
        .await
        .expect("read the inventory");
    assert_eq!(
        after.item(weapon.id).and_then(|item| item.bag_id),
        Some(equipped_bag),
        "the row is in the equipped bag again"
    );

    drop(stream);
    assert!(harness.slot_released().await, "the slot went back");
    placement::remove_world(&harness.pool, harness.world_id)
        .await
        .expect("remove the world");
    harness.stop();
}

#[tokio::test]
async fn a_character_comes_back_where_it_left() {
    if skip() {
        return;
    }
    let Some(harness) = Harness::start().await else {
        return;
    };

    // ---- the first session: walk away from the spawn point, then leave ----------------------
    let version = harness.version(harness.player_token);
    let Some((mut stream, mut client)) = connect_properly(harness.address, &version).await else {
        panic!("the world refused a token the auth service had just issued");
    };
    read_until(&mut stream, &mut client, game::INSTANCE_LOAD_INFO).await;

    // A position report 100 units from the spawn: inside the reference's budget, so it is adopted.
    // `GAME_CMSG_POSITION_UPDATE` is 26 bytes: `{position: Vec2f, plane: u32, unk0: Vec2f, unk1: u32}`,
    // and the world reads the first three fields.
    let walked = (6702.0f32, 4485.0f32);
    let mut body = Vec::new();
    body.extend_from_slice(&walked.0.to_le_bytes());
    body.extend_from_slice(&walked.1.to_le_bytes());
    body.extend_from_slice(&0u32.to_le_bytes()); // plane
    body.extend_from_slice(&[0u8; 12]); // the two trailing fields the world does not read
    say(&mut stream, &mut client, 0x803D, &body).await;

    // The report answers nothing; a ping is how this test knows the server has read it.
    ask(&mut stream, &mut client, 0x800D, 2).await;
    read_until(&mut stream, &mut client, game::PING_REPLY).await;

    drop(stream);
    assert!(
        harness.slot_released().await,
        "the slot went back when the player left"
    );

    // Where it stood is now in the database, which is the only thing that makes the next session
    // different from the first.
    let saved = asura_db::game::position(&harness.pool, harness.character)
        .await
        .expect("read the position")
        .expect("leaving saves a position");
    assert_eq!(saved.map_id, harness.map_id);
    assert_eq!((saved.x, saved.y), walked);

    // ---- the second session: the same character arrives where it left ----------------------
    let token = harness.transfer().await;
    let version = harness.version(token);
    let Some((mut stream, mut client)) = connect_properly(harness.address, &version).await else {
        panic!("the world refused the second travel's token");
    };
    read_until(&mut stream, &mut client, game::INSTANCE_LOAD_INFO).await;

    ask(&mut stream, &mut client, 0x8088, 2).await;
    let frames = read_until(&mut stream, &mut client, game::INSTANCE_LOAD_SPAWN_POINT).await;
    let spawn = frames
        .iter()
        .find(|frame| frame.header == game::INSTANCE_LOAD_SPAWN_POINT)
        .expect("a spawn point");
    let x = f32::from_le_bytes([spawn.body[4], spawn.body[5], spawn.body[6], spawn.body[7]]);
    let y = f32::from_le_bytes([spawn.body[8], spawn.body[9], spawn.body[10], spawn.body[11]]);
    assert_eq!(
        (x, y),
        walked,
        "the client is put back where the character left, not at the map's spawn"
    );

    // The agent is created at the same place, so the two messages do not disagree.
    ask(&mut stream, &mut client, 0x8090, 19).await;
    let frames = read_until(&mut stream, &mut client, game::INSTANCE_LOAD_FINISH).await;
    let agent = frames
        .iter()
        .find(|frame| frame.header == game::WORLD_CREATE_AGENT)
        .expect("an agent");
    assert_eq!(
        f32::from_le_bytes([
            agent.body[10],
            agent.body[11],
            agent.body[12],
            agent.body[13]
        ]),
        walked.0,
        "the agent stands where the spawn point says"
    );

    drop(stream);
    assert!(
        harness.slot_released().await,
        "and the slot went back again"
    );
    placement::remove_world(&harness.pool, harness.world_id)
        .await
        .expect("remove the world");
    harness.stop();
}

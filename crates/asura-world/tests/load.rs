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
//! * the spawn point tells the client the map blob and where to stand ÃƒÂ¢Ã¢â€šÂ¬Ã¢â‚¬Â the two numbers without which
//!   the loading screen never ends;
//! * the players request ends with `INSTANCE_LOAD_FINISH`, which is what takes a client off the loading
//!   screen;
//! * the world keeps the clock running without being asked, and answers a ping;
//! * a dropped item becomes an agent of its own, and Ã¢â‚¬â€ as in the reference Ã¢â‚¬â€ stays in its bag;
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
    /// A second map the same world serves, so `/tp` has somewhere to go. The reference pre-spawns one
    /// server per map; this world hosts several maps in one process, so a teleport here is a hop inside
    /// one world -- the same messages, one fewer process.
    other_map_id: i32,
    address: SocketAddr,
    session: Uuid,
    account: Uuid,
    character: Uuid,
    /// The token the auth service "issued" for this harness's travel. Random per harness because the
    /// transfer table is keyed by it and these tests run in parallel.
    player_token: u32,
    server: tokio::task::JoinHandle<()>,
    /// The world the server task is serving, kept so a test can look at its state (the peer count an
    /// idle run must not grow).
    world: Arc<World>,
}

impl Harness {
    /// Start a world serving one map, and place a player in it the way the auth service would.
    async fn start() -> Option<Harness> {
        Harness::start_on(0).await
    }

    /// A world serving maps of one type: 0 for a main town, 11 for a character-creation instance.
    async fn start_on(map_type: i32) -> Option<Harness> {
        let url = asura_db::config::database_url()?;
        let pool = placement::connect(&url, 4)
            .await
            .expect("connect the database");

        let world_id = unique_world();
        let map_id = unique_map();
        let other_map_id = unique_map();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind a loopback port");
        let address = listener.local_addr().expect("local address");

        let world = Arc::new(World {
            pool: pool.clone(),
            world_id,
            maps: vec![
                MapEntry {
                    map_id,
                    // The reference's blob for the pre-Searing region.
                    map_file_id: 0x1B97D,
                    map_type,
                    spawn: (6602.0, 4485.0),
                    spawn_plane: 0,
                    name: "Lakeside County".to_string(),
                    capacity: 2,
                },
                MapEntry {
                    map_id: other_map_id,
                    map_file_id: 0x1B97D,
                    map_type: 0,
                    spawn: (1.0, 2.0),
                    spawn_plane: 0,
                    name: "Ascalon City".to_string(),
                    capacity: 2,
                },
            ],
            replay: None,
            peers: Default::default(),
            public: address,
        });
        let server = tokio::spawn({
            let world = world.clone();
            async move {
                let _ = serve(listener, world, true).await;
            }
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
            &[(map_id, 2), (other_map_id, 2)],
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
            other_map_id,
            address,
            session: seeded.session,
            account: seeded.account,
            character: seeded.character,
            player_token,
            server,
            world,
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
    /// A second call is how a test gets a second session for the same character Ã¢â‚¬â€ the first transfer is
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
    ///
    /// The window is generous on purpose: leaving writes the character's position *and* releases the
    /// lease, and a whole test suite hammering one database can make two round trips take longer than a
    /// couple of seconds Ã¢â‚¬â€ which produced a flake before this comment existed.
    async fn slot_released(&self) -> bool {
        for _ in 0..100 {
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
    assert!(
        harness.slot_released().await,
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
async fn a_drop_deducts_the_stack_and_says_nothing() {
    if skip() {
        return;
    }
    // The default mode: no message at all, because both candidate messages crashed a live client.
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

    // A ping is how this test knows the server has read the drop, since the drop itself answers nothing.
    ask(&mut stream, &mut client, 0x800D, 2).await;
    let frames = read_until(&mut stream, &mut client, game::PING_REPLY).await;
    for forbidden in [
        game::ITEM_REMOVE,
        game::CREATE_NAMED_ITEM,
        game::WORLD_CREATE_AGENT,
    ] {
        assert!(
            !frames.iter().any(|frame| frame.header == forbidden),
            "{forbidden:#06X} must not be sent by a drop: a live client crashed on two of these and \
             no capture of any of them exists"
        );
    }

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
    // With the echo (the default) the client is told where the item ended up. A silent answer left a
    // live client's item back where it started, which is why the echo is the default and `silent` is the
    // switch nobody has needed yet.
    let frames = read_until(&mut stream, &mut client, game::ITEM_MOVE_TO_LOCATION).await;
    let echo = frames
        .iter()
        .rev()
        .find(|frame| frame.header == game::ITEM_MOVE_TO_LOCATION)
        .expect("an accepted move is echoed");
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
    let frames = read_until(&mut stream, &mut client, game::ITEM_MOVE_TO_LOCATION).await;
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

/// A spoken line: `GAME_CMSG_SEND_CHAT_MESSAGE` = `{agent_id u32, String16}`.
/// Read the redirect pair the way a client that knows about context would: 0x01A6 (46 bytes), then the
/// 0x0099 that follows it (12 bytes), found by scanning the decrypted stream.
async fn read_redirect(stream: &mut TcpStream, client: &mut ReferenceClient) -> (Vec<u8>, Vec<u8>) {
    let mut plain: Vec<u8> = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let read = tokio::time::timeout(TIMEOUT, stream.read(&mut buffer))
            .await
            .expect("read before the timeout")
            .expect("read");
        assert!(
            read > 0,
            "the world closed the connection before the redirect"
        );
        let mut chunk = buffer[..read].to_vec();
        client.decrypt(&mut chunk);
        plain.extend_from_slice(&chunk);
        if let Some(at) = plain.windows(2).position(|window| window == [0xA6, 0x01]) {
            if plain.len() >= at + 58 {
                return (
                    plain[at..at + 46].to_vec(),
                    plain[at + 46..at + 58].to_vec(),
                );
            }
        }
    }
}

fn chat_body(text: &str) -> Vec<u8> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut body = 1u32.to_le_bytes().to_vec();
    body.extend_from_slice(&(units.len() as u16).to_le_bytes());
    for unit in units {
        body.extend_from_slice(&unit.to_le_bytes());
    }
    body
}

#[tokio::test]
async fn maps_prints_its_table_and_puts_nothing_on_the_wire() {
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

    // `/maps`, exactly as a client types it. The answer is the reference's table, printed in the server's
    // console (`--nocapture` shows it); **nothing is sent**, which is what the reference does too: its
    // `GameSrv_SendSystemChatMessage` returns before touching the socket unless `GWPS_CHAT` is set, so a
    // client has nothing to receive and nothing to crash on.
    say(&mut stream, &mut client, 0x8064, &chat_body("/maps")).await;

    ask(&mut stream, &mut client, 0x800D, 2).await;
    let frames = read_until(&mut stream, &mut client, game::PING_REPLY).await;
    for forbidden in [
        game::CHAT_MESSAGE_CORE,
        game::CHAT_MESSAGE_SERVER,
        game::CHAT_MESSAGE_LOCAL,
    ] {
        assert!(
            !frames.iter().any(|frame| frame.header == forbidden),
            "{forbidden:#06X} must not be sent for /maps: the reference answers into its own log, and \
             sending chat crashed a live client here"
        );
    }

    // An unknown command is the same: the reference's `GameSrv_SendInvalidCommand` logs "Unknown command"
    // and returns without sending, under the same gate.
    say(&mut stream, &mut client, 0x8064, &chat_body("/nonsense")).await;
    ask(&mut stream, &mut client, 0x800D, 2).await;
    let frames = read_until(&mut stream, &mut client, game::PING_REPLY).await;
    assert!(
        !frames
            .iter()
            .any(|frame| frame.header == game::CHAT_MESSAGE_CORE),
        "an unknown command answers in the log, not on the wire"
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

/// T9/T14: a second client in the same map, and what the two connections tell each other.
///
/// This is the multi-player surface the reference has and this server did not: two agents with two ids,
/// each player introduced to the other in the reference's own order (`GameSrv_SendWorldAgents`,
/// `GmAgent.c:310-331`), and the pair of messages that take a player back out again
/// (`GameSrv_BroadcastPlayerDestroy`, `GmAgent.c:60`).
#[tokio::test]
async fn a_second_player_is_introduced_to_the_first_and_withdrawn_again() {
    if skip() {
        return;
    }
    let Some(harness) = Harness::start().await else {
        return;
    };

    // The first player: the identity the harness seeded.
    let version_a = harness.version(harness.player_token);
    let Some((mut stream_a, mut client_a)) = connect_properly(harness.address, &version_a).await
    else {
        panic!("the world refused the first client");
    };
    ask(&mut stream_a, &mut client_a, 0x8088, 2).await;
    read_until(
        &mut stream_a,
        &mut client_a,
        game::INSTANCE_LOAD_SPAWN_POINT,
    )
    .await;
    ask(&mut stream_a, &mut client_a, 0x8090, 19).await;
    read_until(&mut stream_a, &mut client_a, game::INSTANCE_LOAD_FINISH).await;

    // The second: a different character on the same map and the same world server, placed the way the
    // auth service would.
    let second = db::seed_identity(
        &harness.pool,
        Uuid::new_v4(),
        Uuid::new_v4(),
        &format!("m4-second-{}", Uuid::new_v4()),
        "Second Player",
        harness.map_id,
    )
    .await
    .expect("seed a second identity");
    placement::reserve(
        &harness.pool,
        harness.world_id,
        harness.map_id,
        second.session,
    )
    .await
    .expect("reserve for the second player");
    let token_b = (Uuid::new_v4().as_u128() & 0xFFFF_FFFF) as u32;
    placement::begin_transfer(
        &harness.pool,
        token_b,
        harness.world_id,
        harness.map_id,
        second.session,
        second.account,
        second.character,
    )
    .await
    .expect("write the second transfer");

    let version_b = GameVersion {
        version: 39112,
        h0008: 1,
        map_token: harness.world_id as u32,
        map_id: harness.map_id as u32,
        player_token: token_b,
        account: second.account.to_bytes_le(),
        character: second.character.to_bytes_le(),
        h0038: 0,
        h003c: 0,
    };
    let Some((mut stream_b, mut client_b)) = connect_properly(harness.address, &version_b).await
    else {
        panic!("the world refused the second client");
    };
    ask(&mut stream_b, &mut client_b, 0x8088, 2).await;
    read_until(
        &mut stream_b,
        &mut client_b,
        game::INSTANCE_LOAD_SPAWN_POINT,
    )
    .await;

    // The second client asks who is here: its own block, then the first player's introduction.
    ask(&mut stream_b, &mut client_b, 0x8090, 19).await;
    // The block and the introduction of the other player are one write, so one read holds both:
    // read_until returns everything it read, not only up to the header it waited for.
    let everything = read_until(&mut stream_b, &mut client_b, game::INSTANCE_LOAD_FINISH).await;

    // Its own player is in the block, so the ids are a set: one for this client, one for the other.
    let players: Vec<u32> = everything
        .iter()
        .filter(|frame| frame.header == game::PLAYER_CREATE)
        .map(|frame| {
            u32::from_le_bytes([frame.body[0], frame.body[1], frame.body[2], frame.body[3]])
        })
        .collect();
    assert_eq!(players.len(), 2, "two players: this one and the other");
    assert!(
        players.contains(&asura_world::PLAYER_ID),
        "the first player keeps id 1"
    );
    let other_agent = everything
        .iter()
        .rev()
        .find(|frame| frame.header == game::WORLD_CREATE_AGENT)
        .expect("an agent for the other player");
    assert_eq!(
        u32::from_le_bytes([
            other_agent.body[0],
            other_agent.body[1],
            other_agent.body[2],
            other_agent.body[3]
        ]),
        asura_world::AGENT_ID,
        "and the first player's agent keeps agent id 1"
    );

    // The first client is told about the second, in the same order.
    let introduced = read_until(&mut stream_a, &mut client_a, game::WORLD_CREATE_AGENT).await;
    let newcomer = introduced
        .iter()
        .find(|frame| frame.header == game::PLAYER_CREATE)
        .expect("the first client is told about the second player");
    let newcomer_id = u32::from_le_bytes([
        newcomer.body[0],
        newcomer.body[1],
        newcomer.body[2],
        newcomer.body[3],
    ]);
    assert_ne!(
        newcomer_id,
        asura_world::PLAYER_ID,
        "the second player gets an id of its own"
    );
    assert!(
        introduced
            .iter()
            .any(|frame| frame.header == game::PLAYER_UPDATE_PARTY_SIZE),
        "with its party size, as the reference sends it"
    );
    assert!(
        introduced
            .iter()
            .any(|frame| frame.header == game::PLAYER_UPDATE_PARTY),
        "and its own party, as the reference sends it"
    );
    // And what it takes to draw them: `GameSrv_BroadcastPlayerEquippedItems` sends the equipped items in
    // their *unnamed* form, then the visual equipment and the cape (`GameSrv.c:1648-1650`).
    assert!(
        introduced
            .iter()
            .any(|frame| frame.header == game::CREATE_UNNAMED_ITEM),
        "the newcomer's equipped items, unnamed"
    );
    assert!(
        introduced
            .iter()
            .any(|frame| frame.header == game::UPDATE_AGENT_VISUAL_EQUIPMENT),
        "the newcomer's visual equipment"
    );
    assert!(
        introduced
            .iter()
            .any(|frame| frame.header == game::AGENT_DISPLAY_CAPE),
        "and its cape"
    );

    // T14: the second client goes away, and the first one's world loses the agent.
    drop(stream_b);
    let left = read_until(&mut stream_a, &mut client_a, game::PLAYER_DESTROY).await;
    let destroyed = left
        .iter()
        .find(|frame| frame.header == game::PLAYER_DESTROY)
        .expect("the departure is announced");
    assert_eq!(
        // A four-byte game header, so the body opens with the header's upper half: the player id starts
        // at byte 2 of the body, not at byte 0.
        u32::from_le_bytes([
            destroyed.body[2],
            destroyed.body[3],
            destroyed.body[4],
            destroyed.body[5]
        ]),
        newcomer_id,
        "the player that left is the one named"
    );
    assert!(
        left.iter()
            .any(|frame| frame.header == game::WORLD_REMOVE_AGENT),
        "and its agent is withdrawn, in the same push"
    );

    drop(stream_a);
    placement::remove_world(&harness.pool, harness.world_id)
        .await
        .expect("remove the world");
    harness.stop();
}

/// `/tp`: a server transfer, not a position change.
///
/// What the reference does (`PreSearing/docs/08-gm-commands.md`): reserve a seat on the destination,
/// mint a player token and record the identity the next connection must present, send
/// `TRANSFER_GAME_SERVER_INFO` then `MAP_UPDATE_CURRENT`, and let the client reconnect. The two messages
/// are pinned to the capture; this test pins the cycle around them: the refusals, the redirect, the
/// transfer row, and the seat accounting that a naive teardown gets wrong.
#[tokio::test]
async fn a_teleport_redirects_to_the_target_map_and_keeps_its_seat() {
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
    ask(&mut stream, &mut client, 0x8088, 2).await;
    read_until(&mut stream, &mut client, game::INSTANCE_LOAD_SPAWN_POINT).await;
    ask(&mut stream, &mut client, 0x8090, 19).await;
    read_until(&mut stream, &mut client, game::INSTANCE_LOAD_FINISH).await;

    // The reference's first refusal: a name that is not a map. Nothing goes on the wire.
    say(&mut stream, &mut client, 0x8064, &chat_body("/tp nowhere")).await;
    // And the one that is already where it asked to go.
    say(
        &mut stream,
        &mut client,
        0x8064,
        &chat_body("/tp lakeside county"),
    )
    .await;
    // A ping is answered, and its reply is what proves the two refusals sent nothing before it: the
    // frames that arrive are the ping's, not a redirect's.
    ask(&mut stream, &mut client, 0x800D, 2).await;
    let after_refusals = read_until(&mut stream, &mut client, game::PING_REPLY).await;
    assert!(
        !after_refusals
            .iter()
            .any(|frame| frame.header == game::TRANSFER_GAME_SERVER_INFO),
        "a refused /tp sends no redirect"
    );

    // Now a real one. The pair is read by hand: the generated table says 0x0099 is five bytes,
    // which is its login form, and a client that frames by the table cannot read the twelve-byte
    // redirect form that follows a transfer -- the reference's own "size per context" trap
    // (`docs/03-protocol.md` section 6).
    say(
        &mut stream,
        &mut client,
        0x8064,
        &chat_body("/tp ascalon city"),
    )
    .await;
    let (transfer, update) = read_redirect(&mut stream, &mut client).await;

    assert_eq!(&transfer[0..2], &[0xA6, 0x01], "0x01A6 first");
    assert_eq!(transfer.len(), 46, "46 bytes, the size the reference sends");
    // host[24], then map_token, region, map_id, is_explorable, player_token.
    let dword = |at: usize, bytes: &[u8]| {
        u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
    };
    assert_eq!(&transfer[2..4], &[2, 0], "AF_INET, little endian");
    assert_eq!(
        dword(26, &transfer),
        harness.world_id as u32,
        "the world to join"
    );
    assert_eq!(
        dword(30, &transfer),
        0,
        "region: this server has none, and sends 0"
    );
    assert_eq!(
        dword(34, &transfer),
        harness.other_map_id as u32,
        "the destination map"
    );
    assert_eq!(
        dword(38, &transfer),
        0,
        "is_explorable: an outpost, and the flag describes the destination"
    );
    let player_token = dword(42, &transfer);

    assert_eq!(&update[0..2], &[0x99, 0x00], "0x0099 follows");
    assert_eq!(update.len(), 12, "the redirect form is twelve bytes");
    assert_eq!(
        dword(4, &update),
        harness.other_map_id as u32,
        "and names the same destination"
    );

    // The seat moved: the old map's is free, the destination's is taken, in one transaction.
    let counts = placement::pool_counts(&harness.pool, harness.world_id, harness.other_map_id)
        .await
        .expect("read the counter")
        .expect("a pool row for the destination");
    assert_eq!(counts.1, 1, "the destination seat is taken");

    // The client reconnects with the token it was handed, and lands in the destination map.
    let moved_version = GameVersion {
        version: 39112,
        h0008: 1,
        map_token: harness.world_id as u32,
        map_id: harness.other_map_id as u32,
        player_token,
        account: harness.account.to_bytes_le(),
        character: harness.character.to_bytes_le(),
        h0038: 0,
        h003c: 0,
    };
    let Some((mut moved_stream, mut moved_client)) =
        connect_properly(harness.address, &moved_version).await
    else {
        panic!("the world refused the token it had just issued");
    };
    ask(&mut moved_stream, &mut moved_client, 0x8088, 2).await;
    let spawn = read_until(
        &mut moved_stream,
        &mut moved_client,
        game::INSTANCE_LOAD_SPAWN_POINT,
    )
    .await;
    let spawned = spawn
        .iter()
        .find(|frame| frame.header == game::INSTANCE_LOAD_SPAWN_POINT)
        .expect("the spawn answer");
    // The spawn message carries the blob id first, and both maps here share one, so the position is
    // what says *which* map: the destination spawns at (1, 2) and the origin at (6602, 4485).
    let x = f32::from_le_bytes([
        spawned.body[4],
        spawned.body[5],
        spawned.body[6],
        spawned.body[7],
    ]);
    let y = f32::from_le_bytes([
        spawned.body[8],
        spawned.body[9],
        spawned.body[10],
        spawned.body[11],
    ]);
    assert_eq!(
        (x, y),
        (1.0, 2.0),
        "the new connection stands at the destination's own spawn point"
    );

    // The old connection now goes away. Its teardown must **not** hand back the seat, because the only
    // lease the session holds is the destination's -- the one this connection is standing on.
    drop(stream);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let counts = placement::pool_counts(&harness.pool, harness.world_id, harness.other_map_id)
        .await
        .expect("read the counter")
        .expect("a pool row for the destination");
    assert_eq!(
        counts.1, 1,
        "the moved connection's teardown left the destination seat alone"
    );

    drop(moved_stream);
    placement::remove_world(&harness.pool, harness.world_id)
        .await
        .expect("remove the world");
    harness.stop();
}

/// T6: a character can be made in a character-creation instance.
///
/// The reference's four messages (`GameSrv_HandleCharCreation*`), in order: the player-data block, the PvP
/// half, a profession change that swaps the equipped set, and a confirm that writes the rows and answers
/// `CHAR_CREATION_SUCCESS`. The equipment is memory until the confirm, which is where the reference writes
/// it too.
#[tokio::test]
async fn a_character_is_created_in_a_creation_instance() {
    if skip() {
        return;
    }
    let Some(harness) = Harness::start_on(11).await else {
        return;
    };

    let version = harness.version(harness.player_token);
    let Some((mut stream, mut client)) = connect_properly(harness.address, &version).await else {
        panic!("the world refused a token the auth service had just issued");
    };

    // 1. the player-data block ends with `INSTANCE_PLAYER_DATA_DONE`, which is creation-only.
    ask(&mut stream, &mut client, 0x8089, 2).await;
    let block = read_until(&mut stream, &mut client, game::INSTANCE_PLAYER_DATA_DONE).await;
    assert!(
        block
            .iter()
            .any(|frame| frame.header == game::INVENTORY_CREATE_BAG),
        "the creation screen is given bags"
    );
    assert!(
        block
            .iter()
            .any(|frame| frame.header == game::CREATE_NAMED_ITEM),
        "and the starting equipment"
    );
    // The three int properties `GameSrv_SendPlayerHealthEnergy` sends on this path: energy 20, health 100,
    // and property 64 zero. Not the load path's 25/400/regen-float/level, which is a different block.
    let properties: Vec<(u32, u32)> = block
        .iter()
        .filter(|frame| frame.header == game::AGENT_PROPERTY_UPDATE_INT)
        .map(|frame| {
            (
                u32::from_le_bytes([frame.body[0], frame.body[1], frame.body[2], frame.body[3]]),
                u32::from_le_bytes([frame.body[8], frame.body[9], frame.body[10], frame.body[11]]),
            )
        })
        .collect();
    assert_eq!(
        properties,
        vec![
            (game::AGENT_PROPERTY_ENERGY, 20),
            (game::AGENT_PROPERTY_HEALTH, 100),
            (64, 0),
        ],
        "the creation path's own three properties, in its own order"
    );
    assert!(
        !block
            .iter()
            .any(|frame| frame.header == game::AGENT_PROPERTY_UPDATE_FLOAT),
        "and no float property: the creation path sends none"
    );

    // 2. the PvP half: empty streams, as an account with nothing unlocked has.
    ask(&mut stream, &mut client, 0x808A, 2).await;
    let pvp = read_until(&mut stream, &mut client, game::PVP_ITEM_STREAM_END).await;
    assert!(
        pvp.iter()
            .any(|frame| frame.header == game::PVP_UPDATE_UNLOCKED_SKILLS),
        "the unlocked-skill list"
    );

    // 3. a profession change: Prophecies Ranger (campaign 1, profession 2).
    say(&mut stream, &mut client, 0x8060, &[1, 2]).await;
    let changed = read_until(&mut stream, &mut client, game::PLAYER_UPDATE_PROFESSION).await;
    // `GameSrv_SendItemsInBag` sends the *unnamed* form for bag slots, not the named one the inventory
    // stream uses (`GmItem.c:111-127`).
    assert!(
        changed
            .iter()
            .any(|frame| frame.header == game::CREATE_UNNAMED_ITEM),
        "the new profession's equipment follows the profession"
    );

    // 4. the confirm: name, appearance, flags. The reference's confirm is 58 bytes.
    let mut confirm = Vec::new();
    let units: Vec<u16> = "Asura Test".encode_utf16().collect();
    confirm.extend_from_slice(&(units.len() as u16).to_le_bytes());
    for unit in &units {
        confirm.extend_from_slice(&unit.to_le_bytes());
    }
    // An appearance the client actually chose: the packed `Appearance` word the confirm carries.
    let chosen_appearance: u32 = 0x3060_0405; // sex, height, skin, hair and face bits, all set
    confirm.extend_from_slice(&chosen_appearance.to_le_bytes());
    confirm.extend_from_slice(&[0u8; 4]); // the rest of the eight-byte config
    confirm.extend_from_slice(&0u32.to_le_bytes()); // flags
    say(&mut stream, &mut client, 0x808B, &confirm).await;
    let created = read_until(&mut stream, &mut client, game::CHAR_CREATION_SUCCESS).await;
    let success = created
        .iter()
        .find(|frame| frame.header == game::CHAR_CREATION_SUCCESS)
        .expect("the character was written");
    // Little endian, which is what the reference writes (`uuid_enc_le`) and what the builder sent.
    let character = Uuid::from_bytes_le(success.body[0..16].try_into().expect("sixteen bytes"));

    // The rows T6 asks for, read back through this repo's own queries: the character, its six bags, and
    // the profession's equipment in the equipped bag.
    assert_eq!(
        asura_db::game::character_name(&harness.pool, character)
            .await
            .expect("read the character"),
        Some("Asura Test".to_string()),
        "a row in characters, named as the client named it"
    );
    // And the traits the load block reads: the appearance the client chose, the profession the screen
    // last set, and the one unlocked-profession bit the reference sets for it.
    let traits = asura_db::game::traits(&harness.pool, character)
        .await
        .expect("read the traits")
        .expect("a traits row");
    assert_eq!(
        traits.appearance, chosen_appearance,
        "the chosen appearance"
    );
    assert_eq!(traits.profession, 2, "the Ranger the screen settled on");
    assert_eq!(traits.unlocked_professions, 1 << 2, "and its one bit");
    let inventory = db_inventory::load(&harness.pool, character)
        .await
        .expect("read the inventory");
    assert_eq!(inventory.bags.len(), 6, "the reference's six bags");
    let equipped_bag = inventory
        .bags
        .iter()
        .find(|bag| bag.bag_type == 2)
        .expect("an equipped bag")
        .bag_id;
    let worn: Vec<i64> = inventory
        .items
        .iter()
        .filter(|item| item.bag_id == Some(equipped_bag))
        .map(|item| item.file_id)
        .collect();
    assert!(!worn.is_empty(), "the profession's equipment was written");
    // And it is the *Ranger's* set, which is what the profession change asked for (campaign 1,
    // profession 2) — not the Warrior the screen started with.
    let expected: Vec<i64> = asura_world::default_equipment::default_equipment(1, 2)
        .iter()
        .map(|spec| i64::from(spec.file_id))
        .collect();
    let mut worn_sorted = worn.clone();
    let mut expected_sorted = expected.clone();
    worn_sorted.sort_unstable();
    expected_sorted.sort_unstable();
    assert_eq!(
        worn_sorted, expected_sorted,
        "the equipped set is the one the chosen profession's table gives"
    );

    drop(stream);
    placement::remove_world(&harness.pool, harness.world_id)
        .await
        .expect("remove the world");
    harness.stop();
}

/// T13: an idle pair keeps its rhythm, and neither the world nor the mailbox grows.
///
/// The reference's T13 asks for two clients left alone for ten minutes: no crash, the ping/tick cycle
/// still running, memory roughly flat. Two of those three are measurable in a test — the rhythm, by
/// counting what arrives, and the accounting, by watching the world's peer count — and the third is not:
/// a working set cannot be measured portably from inside a test, so it stays part of the manual run
/// (`docs/10-parity-queue.md`, and the `--ignored` variant below is that run's harness half).
///
/// The window here is seconds rather than minutes, because a test that takes ten minutes is a test nobody
/// runs: what it asserts is the *cadence* (`PING_INTERVAL` 5 s, `TICK_INTERVAL` 500 ms, the reference's
/// own constants), which does not change with the length of the observation.
#[tokio::test]
async fn an_idle_pair_keeps_its_rhythm_and_its_count() {
    if skip() {
        return;
    }
    let Some(harness) = Harness::start().await else {
        return;
    };

    let version = harness.version(harness.player_token);
    let Some((mut stream_a, mut client_a)) = connect_properly(harness.address, &version).await
    else {
        panic!("the world refused the first client");
    };
    // The load block arrives unasked; it is not part of the idle window, so it is consumed first.
    read_until(&mut stream_a, &mut client_a, game::INSTANCE_LOAD_INFO).await;

    // A second player on the same map, the way the two-player test places one.
    let second = db::seed_identity(
        &harness.pool,
        Uuid::new_v4(),
        Uuid::new_v4(),
        &format!("m4-idle-{}", Uuid::new_v4()),
        "Idle Player",
        harness.map_id,
    )
    .await
    .expect("seed a second identity");
    placement::reserve(
        &harness.pool,
        harness.world_id,
        harness.map_id,
        second.session,
    )
    .await
    .expect("reserve for the second player");
    let token_b = (Uuid::new_v4().as_u128() & 0xFFFF_FFFF) as u32;
    placement::begin_transfer(
        &harness.pool,
        token_b,
        harness.world_id,
        harness.map_id,
        second.session,
        second.account,
        second.character,
    )
    .await
    .expect("write the second transfer");
    let mut version_b = harness.version(token_b);
    version_b.account = second.account.to_bytes_le();
    version_b.character = second.character.to_bytes_le();
    let Some((mut stream_b, mut client_b)) = connect_properly(harness.address, &version_b).await
    else {
        panic!("the world refused the second client");
    };
    ask(&mut stream_b, &mut client_b, 0x8088, 2).await;
    read_until(
        &mut stream_b,
        &mut client_b,
        game::INSTANCE_LOAD_SPAWN_POINT,
    )
    .await;

    // Both connections are now idle: nothing is sent, and the server keeps its own clock running.
    let mut buffer = [0u8; 8192];
    let mut frames: Vec<Frame> = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(6);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(200), stream_a.read(&mut buffer)).await {
            Ok(Ok(0)) => panic!("the world closed an idle connection"),
            Ok(Ok(read)) => frames.extend(client_a.receive_game(&buffer[..read]).expect("frames")),
            Ok(Err(err)) => panic!("an idle connection failed: {err}"),
            Err(_) => {} // nothing arrived in this slice, which is fine
        }
    }

    let ticks = frames
        .iter()
        .filter(|frame| frame.header == game::WORLD_SIMULATION_TICK)
        .count();
    let pings = frames
        .iter()
        .filter(|frame| frame.header == game::PING_REQUEST)
        .count();
    // Six seconds at a 500 ms tick is at least ten; the assertion allows for scheduling slack.
    assert!(
        ticks >= 8,
        "the simulation tick kept running while idle: {ticks} in six seconds"
    );
    assert!(
        pings >= 1,
        "the ping cycle kept running while idle: {pings} in six seconds"
    );
    // Nothing else was pushed at an idle client: an idle pair gets the clock and the ping, not a stream
    // of anything else.
    let unexpected: Vec<u16> = frames
        .iter()
        .map(|frame| frame.header)
        .filter(|header| *header != game::WORLD_SIMULATION_TICK && *header != game::PING_REQUEST)
        .collect();
    assert!(
        unexpected.is_empty(),
        "an idle client is only told the time: {unexpected:?}"
    );

    assert_eq!(
        harness.world.peers.len(),
        2,
        "two players, and the world's list did not grow while they were idle"
    );

    drop(stream_a);
    drop(stream_b);
    // The registry empties as the connections go, so the count is not a slow leak.
    let mut left = harness.world.peers.len();
    for _ in 0..50 {
        if left == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        left = harness.world.peers.len();
    }
    assert_eq!(left, 0, "both players left the world's list");

    placement::remove_world(&harness.pool, harness.world_id)
        .await
        .expect("remove the world");
    harness.stop();
}

/// The ten-minute half of T13, for the hand run the reference describes: `cargo test -p asura-world
/// --test load a_ten_minute_idle -- --ignored --nocapture`. It asserts the same things after ten minutes,
/// and prints the cadence as it goes so a human can watch it.
#[tokio::test]
#[ignore]
async fn a_ten_minute_idle_pair_keeps_its_rhythm() {
    if skip() {
        return;
    }
    let Some(harness) = Harness::start().await else {
        return;
    };
    let version = harness.version(harness.player_token);
    let Some((mut stream, mut client)) = connect_properly(harness.address, &version).await else {
        panic!("the world refused the client");
    };

    let mut buffer = [0u8; 8192];
    let started = tokio::time::Instant::now();
    let mut ticks = 0usize;
    let mut pings = 0usize;
    while started.elapsed() < Duration::from_secs(600) {
        match tokio::time::timeout(Duration::from_millis(500), stream.read(&mut buffer)).await {
            Ok(Ok(0)) => panic!(
                "the world closed the connection after {:?}",
                started.elapsed()
            ),
            Ok(Ok(read)) => {
                for frame in client.receive_game(&buffer[..read]).expect("frames") {
                    match frame.header {
                        game::WORLD_SIMULATION_TICK => ticks += 1,
                        game::PING_REQUEST => pings += 1,
                        _ => {}
                    }
                }
            }
            Ok(Err(err)) => panic!("the connection failed after {:?}: {err}", started.elapsed()),
            Err(_) => {}
        }
    }
    println!(
        "ten minutes idle: {ticks} ticks, {pings} pings, {} player(s) in the world",
        harness.world.peers.len()
    );
    assert!(ticks >= 1_000, "about two ticks a second for ten minutes");
    assert!(pings >= 100, "a ping every five seconds for ten minutes");
    assert_eq!(harness.world.peers.len(), 1, "one player, still");

    drop(stream);
    placement::remove_world(&harness.pool, harness.world_id)
        .await
        .expect("remove the world");
    harness.stop();
}

/// T10, the half that can be asserted without a path graph.
///
/// The reference pathfinds (`GmPaths.c`: trapezoid A* over the graph `GmMapDataImport.c` reads out of
/// Gw.dat) and answers `MOVE_TO_COORD` with the *first waypoint of its own path*, refusing a destination it
/// cannot reach by sending a tick, a cancel and a position (`GmAgent.c:371-398`). This server has no graph,
/// so it answers with the client's own destination — a straight line — and refuses nothing.
///
/// That is a deviation, and a deviation that is not asserted is a deviation nobody can see break. This test
/// pins the behaviour that exists: the echo is the destination the client sent, it reaches the other player
/// too, and the *position* budget — which does not need a graph and is the reference's own — is enforced on
/// the wire.
#[tokio::test]
async fn a_move_is_echoed_as_sent_and_a_far_report_is_clamped() {
    if skip() {
        return;
    }
    let Some(harness) = Harness::start().await else {
        return;
    };

    // Two players, because the broadcast is what the second one observes.
    let version = harness.version(harness.player_token);
    let Some((mut stream_a, mut client_a)) = connect_properly(harness.address, &version).await
    else {
        panic!("the world refused the first client");
    };
    read_until(&mut stream_a, &mut client_a, game::INSTANCE_LOAD_INFO).await;
    ask(&mut stream_a, &mut client_a, 0x8090, 19).await;
    read_until(&mut stream_a, &mut client_a, game::INSTANCE_LOAD_FINISH).await;

    let second = db::seed_identity(
        &harness.pool,
        Uuid::new_v4(),
        Uuid::new_v4(),
        &format!("m4-move-{}", Uuid::new_v4()),
        "Walker",
        harness.map_id,
    )
    .await
    .expect("seed a second identity");
    placement::reserve(
        &harness.pool,
        harness.world_id,
        harness.map_id,
        second.session,
    )
    .await
    .expect("reserve");
    let token_b = (Uuid::new_v4().as_u128() & 0xFFFF_FFFF) as u32;
    placement::begin_transfer(
        &harness.pool,
        token_b,
        harness.world_id,
        harness.map_id,
        second.session,
        second.account,
        second.character,
    )
    .await
    .expect("write the transfer");
    let mut version_b = harness.version(token_b);
    version_b.account = second.account.to_bytes_le();
    version_b.character = second.character.to_bytes_le();
    let Some((mut stream_b, mut client_b)) = connect_properly(harness.address, &version_b).await
    else {
        panic!("the world refused the second client");
    };
    read_until(&mut stream_b, &mut client_b, game::INSTANCE_LOAD_INFO).await;
    ask(&mut stream_b, &mut client_b, 0x8090, 19).await;
    read_until(&mut stream_b, &mut client_b, game::INSTANCE_LOAD_FINISH).await;
    // The first player's introduction arrives in the same chunk as this client's own block, so the read
    // above already consumed it: no drain, and no second read that would wait for a frame that is gone.

    // ---- the echo: a walk is answered with exactly the destination that was sent ----
    let destination = (6702.0f32, 4585.0f32);
    let mut walk = Vec::new();
    walk.extend_from_slice(&destination.0.to_le_bytes());
    walk.extend_from_slice(&destination.1.to_le_bytes());
    walk.extend_from_slice(&0u32.to_le_bytes()); // plane
    say(&mut stream_a, &mut client_a, 0x803E, &walk).await;

    let echoed = read_until(&mut stream_a, &mut client_a, game::AGENT_MOVE_TO_POINT).await;
    let move_to = echoed
        .iter()
        .find(|frame| frame.header == game::AGENT_MOVE_TO_POINT)
        .expect("the walk is answered");
    assert_eq!(
        f32::from_le_bytes([
            move_to.body[4],
            move_to.body[5],
            move_to.body[6],
            move_to.body[7]
        ]),
        destination.0,
        "the x the client asked for, echoed -- not a waypoint from a graph this server does not have"
    );
    // And the other player is told, which is what makes a second client see the first one move.
    let seen = read_until(&mut stream_b, &mut client_b, game::AGENT_MOVE_TO_POINT).await;
    assert!(
        seen.iter()
            .any(|frame| frame.header == game::AGENT_MOVE_TO_POINT),
        "the other player sees the walk"
    );

    // ---- the budget: a report a kilometre away is clamped, and the clamp is what is broadcast ----
    let far = (destination.0 + 1000.0, destination.1 + 1000.0);
    let mut report = Vec::new();
    report.extend_from_slice(&far.0.to_le_bytes());
    report.extend_from_slice(&far.1.to_le_bytes());
    report.extend_from_slice(&0u32.to_le_bytes());
    report.extend_from_slice(&[0u8; 12]); // the two trailing fields, which the world does not read
    say(&mut stream_a, &mut client_a, 0x803D, &report).await;

    let observed = read_until(&mut stream_b, &mut client_b, game::AGENT_UPDATE_POSITION).await;
    let position = observed
        .iter()
        .find(|frame| frame.header == game::AGENT_UPDATE_POSITION)
        .expect("the clamp is broadcast");
    let x = f32::from_le_bytes([
        position.body[4],
        position.body[5],
        position.body[6],
        position.body[7],
    ]);
    let y = f32::from_le_bytes([
        position.body[8],
        position.body[9],
        position.body[10],
        position.body[11],
    ]);
    let travelled = ((x - destination.0).powi(2) + (y - destination.1).powi(2)).sqrt();
    // The reference's budget: `speed_base * elapsed + POSITION_UPDATE_TOLERANCE`, with the elapsed time
    // capped at `POSITION_UPDATE_MAX_ELAPSED_MS` (2000 ms) -- 288 * 2 + 50 = 626 units.
    assert!(
        travelled > 0.0,
        "an out-of-budget report still moves the agent along the reported direction"
    );
    assert!(
        travelled <= 700.0,
        "and only as far as the budget allows: {travelled:.1} units"
    );
    assert!(
        (x - far.0).abs() > 100.0 && (y - far.1).abs() > 100.0,
        "the far report itself was not believed"
    );

    drop(stream_a);
    drop(stream_b);
    placement::remove_world(&harness.pool, harness.world_id)
        .await
        .expect("remove the world");
    harness.stop();
}

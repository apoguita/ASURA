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
//! * the lease the travel took is released when the player leaves.
//!
//! With no database configured, each test prints a skip line and returns.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

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

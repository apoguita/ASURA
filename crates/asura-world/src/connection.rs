//! One gameplay connection: hello, load, gameplay, goodbye.
//!
//! The server's `ConnectedClient` - the counterpart of Py4GW_Stealth's `client.py`. A socket is
//! classified, its key exchange is completed, the token pair is claimed against a transfer, the
//! load block goes out unasked, and then [`run`] owns the socket: the world tick, the ping, the
//! client's messages (through `dispatch.rs`) and the broadcasts other players push at it.
//!
//! The player record (`player.rs`) is what a connection drives, and it is where the character, the
//! inventory, the agents and the instance they are in currently live. That is the shape the
//! `Instance` object would replace: the map state belongs to the map, not to the socket. Recorded,
//! not fixed here.

use crate::*;

/// How often the world's clock is sent.
///
/// Measured from the live capture (`recordings/client/feature/movement/movement-sniff.log`): a `0x001E` every ~100 ms, carrying the
/// real elapsed milliseconds since the previous one (median 81 ms). The frame is the reference's own
/// 6-byte `GameSrv_UpdateWorldSimulationTick` (`GameMsg.h:521`); only the cadence is the measurement, and
/// the reference's floor is 500 ms.
pub const TICK_INTERVAL: Duration = Duration::from_millis(100);

/// How often the server asks the client whether it is still there. The reference's constant is 5000 ms
/// (`GAME_SRV_TIME_BETWEEN_PING_MS`).
pub const PING_INTERVAL: Duration = Duration::from_secs(5);

/// The ping value the reference answers with. It is a constant, not a measurement: GWPS sends `20`.
pub const PING_VALUE: u32 = 20;

/// Accept gameplay connections until the listener fails.
pub async fn serve(listener: TcpListener, world: Arc<World>, trace: bool) -> io::Result<()> {
    let address = listener.local_addr()?;
    eprintln!(
        "asura-world: listening on {address} for world {} ({} map(s))",
        world.world_id,
        world.maps.len()
    );
    loop {
        let (stream, peer) = listener.accept().await?;
        let world = Arc::clone(&world);
        let connection =
            tokio::spawn(async move { handle_connection(stream, &world, trace).await });
        // The connection is reaped rather than detached. A task that dies of a *panic* leaves this
        // process alive, so from outside "the server crashed" and "one connection panicked" look the
        // same - and the panic's own message is the only thing that says which it was.
        tokio::spawn(async move {
            match connection.await {
                Ok(Ok(())) => {
                    if trace {
                        eprintln!("world: {peer} left");
                    }
                }
                Ok(Err(err)) => eprintln!("world: {peer}: {err}"),
                Err(err) => {
                    if err.is_panic() {
                        eprintln!(
                            "world: {peer}: connection task PANICKED: {}",
                            panic_message(err)
                        );
                    } else {
                        eprintln!("world: {peer}: connection task ended: {err}");
                    }
                }
            }
        });
    }
}

/// The message a panicking task carried, if it carried one a human can read.
pub fn panic_message(err: tokio::task::JoinError) -> String {
    let payload = err.into_panic();
    payload
        .downcast_ref::<&str>()
        .map(|message| (*message).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic with no message".to_string())
}

/// One connection's cipher state and the decrypted bytes not yet framed.
pub struct Session {
    pub established: Established,
    pub plaintext: Vec<u8>,
}

impl Session {
    pub fn new(established: Established) -> Self {
        Session {
            established,
            plaintext: Vec::new(),
        }
    }

    /// Take `count` bytes, decrypt them in place, and append them to the plaintext buffer.
    pub fn absorb(&mut self, bytes: &[u8]) {
        let start = self.plaintext.len();
        self.plaintext.extend_from_slice(bytes);
        self.established
            .incoming
            .apply(&mut self.plaintext[start..]);
    }
}

/// Drive one gameplay connection from the first byte to the close.
pub async fn handle_connection(
    mut stream: TcpStream,
    world: &World,
    trace: bool,
) -> Result<(), GameError> {
    // ---- 1. what kind of connection is this? ------------------------------------------------
    let mut header = [0u8; 4];
    stream.read_exact(&mut header).await?;
    match version::classify(&header) {
        Ok(version::ConnectionKind::Game) => {}
        Ok(other) => {
            // An auth or control connection belongs to another program. Refusing it by name is the
            // point of classifying before anything else.
            eprintln!("world: a {other:?} connection arrived on the game port; closing it");
            return Err(GameError::NotAGameConnection(
                version::VersionError::Unknown {
                    header: u32::from_le_bytes(header),
                },
            ));
        }
        Err(err) => return Err(GameError::NotAGameConnection(err)),
    }

    // ---- 2. the version message -------------------------------------------------------------
    let mut message = vec![0u8; GAME_CMSG_VERSION_LEN];
    message[..4].copy_from_slice(&header);
    stream.read_exact(&mut message[4..]).await?;
    let (game_version, _) = GameVersion::decode(&message).map_err(GameError::BadVersion)?;
    if trace {
        eprintln!(
            "world <- game version build={} world={} map={} player token={:#010X}",
            game_version.version,
            game_version.map_token,
            game_version.map_id,
            game_version.player_token
        );
    }

    // ---- 3. the token pair is the credential ------------------------------------------------
    let refusal = GameError::UnknownTransfer {
        map_token: game_version.map_token,
        player_token: game_version.player_token,
    };
    let transfer =
        placement::claim_transfer(&world.pool, world.world_id, game_version.player_token)
            .await
            .map_err(GameError::Database)?
            .ok_or(GameError::UnknownTransfer {
                map_token: game_version.map_token,
                player_token: game_version.player_token,
            })?;

    // The character the client claims must be the one the transfer was issued for. This is the check
    // the reference makes (`AuthSrv_DoPostHandshake`); skipping it would let any client that knows a
    // live token plays as any character on the account.
    if game_version.character != transfer.character_id.to_bytes_le() {
        return Err(refusal);
    }

    let map_id = transfer.map_id;
    // A client whose character stands in a map this world was **not** started with: with `--on-demand`
    // the map is loaded here and the connection goes on, instead of the client being kicked with
    // "this world does not host map X". That is the whole point of the switch - a login into an
    // unhosted map and a /tp into one are the same refusal when it is off, and the same demand-load
    // when it is on.
    let map = match world.map(map_id) {
        Some(map) => map,
        None if world.load_on_demand => {
            match load_map_on_demand(world, map_id)
                .await
                .map_err(GameError::Database)?
            {
                Some(map) => map,
                None => return Err(GameError::UnknownMap(map_id)),
            }
        }
        None => return Err(GameError::UnknownMap(map_id)),
    };

    let Some(name) = db_game::character_name(&world.pool, transfer.character_id)
        .await
        .map_err(GameError::Database)?
    else {
        return Err(GameError::UnknownCharacter);
    };

    // ---- the character's inventory ------------------------------------------------------------
    // A character with no bags has never been played, and the reference gives it the default set before
    // it sends anything (`GameSrv_CreateDefaultBags`). A client with no inventory model dies when it draws
    // the character it was just given, so this happens before the load block and not lazily.
    let mut inventory = db_inventory::load(&world.pool, transfer.character_id)
        .await
        .map_err(GameError::Database)?;
    if inventory.bags.is_empty() {
        let seeded = db_inventory::seed_default(&world.pool, transfer.character_id)
            .await
            .map_err(GameError::Database)?;
        if seeded {
            inventory = db_inventory::load(&world.pool, transfer.character_id)
                .await
                .map_err(GameError::Database)?;
            eprintln!(
                "world: seeded the default inventory for {name:?} ({} bags, {} items)",
                inventory.bags.len(),
                inventory.items.len()
            );
        }
    }

    // Where the character stands: where it left, if it left from this map.
    let saved = db_game::position(&world.pool, transfer.character_id)
        .await
        .map_err(GameError::Database)?;
    let standing = Standing::resume(&map, saved);

    eprintln!(
        "world: {name:?} joined map {} (file id {}) on world {}, standing at ({:.1}, {:.1}) plane \
         {} with {} bag(s) and {} item(s)",
        map.map_id,
        map.map_file_id,
        world.world_id,
        standing.position.0,
        standing.position.1,
        standing.plane,
        inventory.bags.len(),
        inventory.items.len()
    );

    // ---- 4. the key exchange ----------------------------------------------------------------
    let mut client_seed = vec![0u8; CLIENT_SEED_LEN];
    stream.read_exact(&mut client_seed).await?;
    let mut master_secret = [0u8; MASTER_SECRET_LEN];
    getrandom::getrandom(&mut master_secret).map_err(|err| io::Error::other(err.to_string()))?;
    let established =
        handshake::accept(&client_seed, &master_secret).map_err(GameError::Handshake)?;
    stream.write_all(&established.reply).await?;
    if trace {
        eprintln!("world -> server seed, scrambled from here on");
    }

    let mut session = Session::new(established);
    // What this connection knows about itself: whose character it is, where it stands, and the agents it
    // has created. The ids are allocated here rather than being the constants they used to be: two
    // players in one map need two of each, and everything the reference sends a player about itself
    // carries them.
    let equipped = inventory.equipped_item_ids();
    let (outbox, mut inbox) = tokio::sync::mpsc::unbounded_channel();
    let (player_id, agent_id) = world.peers.join(Peer {
        player_id: 0,
        agent_id: 0,
        name: name.clone(),
        standing,
        equipped,
        outbox,
    });
    let map_type = map.map_type;
    let mut player = Player {
        character: transfer.character_id,
        player_id,
        instance: map,
        name,
        inventory,
        agents: Agents::new(
            agent_id,
            standing.position,
            standing.plane,
            game::PLAYER_SPEED_BASE,
        ),
        session_id: transfer.session_id,
        account_id: transfer.account_id,
        moved: false,
        // A creation instance is its own map type (`MapType_CharacterCreation`, 11): a connection that
        // arrived in one is there to make a character, and one that arrived in anything else cannot.
        creation: if map_type == MAP_TYPE_CHARACTER_CREATION {
            Some(Creation::start())
        } else {
            None
        },
        traits: db_game::traits(&world.pool, transfer.character_id)
            .await
            .map_err(GameError::Database)?
            .unwrap_or_default(),
        started: Instant::now(),
        move_stats: MoveStats::default(),
    };
    if move_trace_on() {
        eprintln!(
            "move[spawn] agent {} starts on map {} at (x={:.1}, y={:.1}, plane={}) with speed {} \
             (the client will walk from here)",
            player.GetAgentID(),
            player.instance.map_id,
            player.agents.player.position.0,
            player.agents.player.position.1,
            player.agents.player.plane,
            game::PLAYER_SPEED_BASE
        );
    }

    // ---- 5. the load block ------------------------------------------------------------------
    // The reference sends these four the moment it has the connection, without waiting to be asked.
    let block = match &world.replay {
        Some(replay) if replay.pick.wants(crate::replay::BlockName::Load) => replay.load.clone(),
        _ => load_block(&player.instance, player.GetName(), player.GetAgentID()),
    };
    send(&mut stream, &mut session, block, trace).await?;

    // ---- 6/7. the asks, then the session ----------------------------------------------------
    // Run to its end however it ends. A session that dies of a framing error is still a player that left,
    // and both things that happen on the way out - the position and the pool slot - belong to the leaving
    // and not to the ending: skipping them for the untidy exits would leak a slot until the reaper and
    // lose where the character stood.
    let outcome = run(
        &mut stream,
        &mut session,
        world,
        &mut player,
        &mut inbox,
        trace,
    )
    .await;

    // The player left: everyone else's world loses the agent before the slot goes back, so a client that
    // is being told about the departure never sees an agent whose seat is already free.
    if world.peers.leave(player.player_id) {
        world
            .peers
            .broadcast_all(&farewell(player.GetPlayerNumber(), player.GetAgentID()));
    }

    // Where the player was when it left is what it comes back to. Written *before* the lease goes back,
    // and best-effort: a failed save must not turn an ordinary logout into an error, and losing a
    // position is cheaper than refusing a player its way out.
    let left = player.agents.player.position;
    let left_plane = player.agents.player.plane;
    if let Err(err) = db_game::record_position(
        &world.pool,
        player.character,
        player.instance.map_id,
        left.0,
        left.1,
        left_plane as i16,
    )
    .await
    {
        eprintln!("world: could not save the position of the leaving player: {err}");
    }

    // The player left. The slot goes back to the pool now rather than when a reaper notices: the
    // player is gone, and a pool that only empties on a reaper would refuse a legitimate travel after
    // an ordinary logout.
    //
    // **Unless it was moved.** `/tp` reserves the *destination*'s seat for this session in the same
    // transaction that gives up this one, so the only lease the session holds by the time this runs is
    // the one the client is about to arrive on. Releasing it would free a seat a player is standing in -
    // the reference's own mirror of this race is recorded in `PreSearing/docs/08-gm-commands.md`.
    if player.moved {
        eprintln!(
            "world: {:?} was moved to map {}; the seat it holds is the destination's",
            player.name, player.instance.map_id
        );
    } else if let Err(err) = placement::release(&world.pool, transfer.session_id).await {
        eprintln!("world: could not release the lease of the leaving player: {err}");
    }
    eprintln!(
        "world: {:?} left map {} from ({:.1}, {:.1}) plane {left_plane}, saved",
        player.name, player.instance.map_id, left.0, left.1
    );
    if move_trace_on() {
        let stats = player.move_stats;
        eprintln!(
            "move[summary] {} walk request(s) (pathfinding), {} position report(s) (manual walk): \
             {} accepted, {} clamped, {} refused, {} plane change(s); {} tick(s) walked, \
             largest client/server gap {:.1} units",
            stats.walk_requests,
            stats.reports,
            stats.accepted,
            stats.clamped,
            stats.refused,
            stats.plane_changes,
            stats.ticks,
            stats.max_drift
        );
    }
    outcome
}

/// The conversation itself: answer what arrives until the client goes away, a message cannot be framed,
/// or the socket fails.
pub async fn run(
    stream: &mut TcpStream,
    session: &mut Session,
    world: &World,
    player: &mut Player,
    inbox: &mut tokio::sync::mpsc::UnboundedReceiver<Vec<Frame>>,
    trace: bool,
) -> Result<(), GameError> {
    let mut tick = tokio::time::interval(TICK_INTERVAL);
    // `GameSrv_WorldTick` announces the real elapsed time between frames, so the clock starts here.
    let mut last_tick = Instant::now();
    let mut ping = tokio::time::interval(PING_INTERVAL);
    let mut buffer = [0u8; 8192];

    'session: loop {
        tokio::select! {
            biased;

            // Frames another player's connection pushed at us. This is the whole of the broadcast
            // mechanism: the reference writes into every other connection's buffer, and here every
            // other connection's task writes into this channel.
            Some(frames) = inbox.recv() => {
                send(stream, session, frames, trace).await?;
            }

            read = stream.read(&mut buffer) => {
                let read = read?;
                if read == 0 {
                    break; // the client closed its side
                }
                session.absorb(&buffer[..read]);

                loop {
                    // Framed by the generated game table, field by field: a counted field carries its
                    // own length, so a message is exactly as long as it says. What arrives is framed
                    // before it is answered, and a header the table does not know ends the session
                    // rather than consuming the wrong number of bytes.
                    match Frame::decode_game_message(&session.plaintext, true) {
                        Ok((frame, used)) => {
                            session.plaintext.drain(..used);
                            crate::capture::record("CToS", frame.header, &frame.encode());
                            if trace {
                                eprintln!(
                                    "world <- {:#06X} ({used} bytes): {}",
                                    frame.header,
                                    hex_preview(&frame.encode())
                                );
                            }
                            if !answer(stream, session, frame, world, player, trace).await? {
                                break 'session;
                            }
                        }
                        Err(FrameError::Incomplete { .. }) => break,
                        Err(err) => return Err(GameError::Framing(err)),
                    }
                }
            }

            _ = tick.tick() => {
                // `GameSrv_WorldTick`: announce the elapsed time, then move every agent along its own
                // walk. The announcement goes to this connection only -- each connection keeps its own
                // ticker here, where the reference has one loop for the whole server, so every client
                // still hears one tick per interval.
                // The announced number is the **real** elapsed time since the previous tick, which is
                // what `GameSrv_WorldTick` sends (`srv->current_frame_time - srv->last_world_tick`).
                let elapsed = last_tick.elapsed();
                last_tick = Instant::now();
                let delta_ms = u32::try_from(elapsed.as_millis()).unwrap_or(u32::MAX);
                let delta = elapsed.as_secs_f32();
                let was_moving = 0.0 < player.agents.player.motion.speed;
                let before = player.agents.player.position;
                let before_plane = player.agents.player.plane;
                let crossed = player.agents.player.advance(delta);
                if move_trace_on() {
                    player.move_stats.ticks += 1;
                    if was_moving {
                        let moved = {
                            let dx = player.agents.player.position.0 - before.0;
                            let dy = player.agents.player.position.1 - before.1;
                            (dx * dx + dy * dy).sqrt()
                        };
                        let expected = game::PLAYER_SPEED_BASE * delta;
                        eprintln!(
                            "move[tick] walked {moved:.1} units (expected {expected:.1} at \
                             {:.0}/s), agent (x={:.1}, y={:.1}, plane={})",
                            game::PLAYER_SPEED_BASE,
                            player.agents.player.position.0,
                            player.agents.player.position.1,
                            player.agents.player.plane
                        );
                        if player.agents.player.plane != before_plane {
                            player.move_stats.plane_changes += 1;
                            eprintln!(
                                "move[jump] the walk changed the agent's plane {before_plane} -> {} \
                                 at (x={:.1}, y={:.1}) - the client will place it on that plane now",
                                player.agents.player.plane,
                                player.agents.player.position.0,
                                player.agents.player.position.1
                            );
                        }
                    }
                    if let Some(waypoint) = &crossed {
                        eprintln!(
                            "move[tick] crossed waypoint {}/{} (x={:.1}, y={:.1}, plane={}, trap={}); \
                             agent now (x={:.1}, y={:.1}, plane={})",
                            player.agents.player.motion.waypoint_idx,
                            player.agents.player.motion.waypoints.len(),
                            waypoint.pos.x,
                            waypoint.pos.y,
                            waypoint.pos.plane,
                            waypoint.trap_id,
                            player.agents.player.position.0,
                            player.agents.player.position.1,
                            player.agents.player.plane
                        );
                    } else if was_moving && player.agents.player.motion.speed == 0.0 {
                        eprintln!(
                            "move[tick] arrival at (x={:.1}, y={:.1}, plane={}); the walk is over",
                            player.agents.player.position.0,
                            player.agents.player.position.1,
                            player.agents.player.plane
                        );
                    }
                }
                let mut frames = vec![game::world_simulation_tick(delta_ms)];
                if let Some(waypoint) = crossed {
                    // A crossed waypoint is a new destination, and the reference broadcasts it: `every`
                    // client is drawing this agent from the last destination it was told
                    // (`GameSrv_UpdateAgentDestination` -> `GameSrv_BroadcastAgentMoveToPoint`).
                    let walk = game::agent_move_to_point(
                        player.GetAgentID(),
                        (waypoint.pos.x, waypoint.pos.y),
                        waypoint.pos.plane,
                        player.agents.player.plane,
                    );
                    world
                        .peers
                        .broadcast(player.player_id, std::slice::from_ref(&walk));
                    frames.push(walk);
                }
                send(stream, session, frames, trace).await?;
                // What the world believes about this player advances with it, so a joining client is told
                // where a walker is now rather than where it was when it asked.
                world.peers.update(player.player_id, player.standing());
            }

            _ = ping.tick() => {
                send(stream, session, vec![game::ping_request()], trace).await?;
            }
        }
    }
    Ok(())
}

/// The first bytes of a message, as hex, for the trace.
///
/// When a client dies mid-load, the only evidence left is what it sent and what it was sent. A header
/// and a size say which message; the first bytes say whether its contents were what the reference
/// would have written.
pub fn hex_preview(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take(24)
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Scramble and write frames.
pub async fn send(
    stream: &mut TcpStream,
    session: &mut Session,
    frames: Vec<Frame>,
    trace: bool,
) -> Result<(), GameError> {
    for frame in frames {
        let mut bytes = frame.encode();
        crate::capture::record("StoC", frame.header, &bytes);
        if trace {
            eprintln!("world -> {:#06X} ({} bytes)", frame.header, bytes.len());
        }
        session.established.outgoing.apply(&mut bytes);
        stream.write_all(&bytes).await?;
    }
    Ok(())
}

/// An item and where it is being put: the three fields `GAME_CMSG_ITEM_MOVE` carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub item_id: i32,
    pub bag_id: i32,
    pub slot: i32,
}

/// The client asks to be pinged back.
pub async fn handle_ping_request(
    stream: &mut TcpStream,
    session: &mut Session,
    trace: bool,
) -> Result<bool, GameError> {
    send(stream, session, vec![game::ping_reply(PING_VALUE)], trace).await?;
    Ok(true)
}

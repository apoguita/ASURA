//! The auth service: the program the game talks to when it logs in.
//!
//! Owns: the login endpoint (key exchange, login, character list), sessions, and telling the client
//! which world to join. Does NOT own placement, character state, instances, or the gameplay
//! connection — those belong to the world servers (`docs/01-scope.md`).
//!
//! AzerothCore parallel: their `authserver` — realmlist, session key, and nothing else.
//!
//! The connection follows the order `docs/03-protocol.md` records, and each step is a module in
//! `asura-protocol`:
//!
//! ```text
//! 1. first four bytes      client::version      what kind of connection is this
//! 2. the version message   client::version      plaintext, 16 bytes
//! 3. the key exchange      client::handshake    two plaintext seeds, then everything is scrambled
//! 4. frames                client::frame        header plus a body the header's size fixes
//! 5. login                 client::messages + asura-db::login
//! ```
//!
//! Two behaviours worth stating, because they are rules from the reference and not preferences:
//!
//! * **A connection that is not understood is closed, not guessed at.** GWPS logs the bytes and drops
//!   the socket; `VersionError::Unknown` does the same here.
//! * **A login that does not resolve is answered with the client's auth error**, in a request
//!   response, rather than by silence or by a drop: the client shows an error it can describe, and
//!   the session stays closed (`asura_db::login::find_session` returns `None`).

use std::io;
use std::time::Duration;

use asura_db::login as db_login;
use asura_db::PgPool;
use asura_protocol::client::handshake::{self, CLIENT_SEED_LEN, MASTER_SECRET_LEN};
use asura_protocol::client::messages::{self, PortalAccountLogin, RequestResponse, STATUS_OK};
use asura_protocol::client::tables;
use asura_protocol::client::version::{self, VersionError, AUTH_CMSG_VERSION_LEN};
use asura_protocol::client::{Frame, FrameError};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use uuid::Uuid;

/// How long a connection may sit without sending anything before it is dropped.
///
/// Not from the reference — GWPS relies on the operating system's timeouts — but a login connection
/// that stalls holds a task and a pool connection, and losing it is free: the client reconnects.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(120);

/// How many bytes to read from the socket at once.
const READ_CHUNK: usize = 4096;

/// Why a connection ended.
#[derive(Debug)]
pub enum ConnectionError {
    /// The socket failed.
    Io(io::Error),
    /// The first four bytes name no connection type this service knows.
    UnknownConnection(VersionError),
    /// A well-formed connection that belongs to a different program: a world server, not the auth
    /// service.
    NotAnAuthConnection { kind: version::ConnectionKind },
    /// The first message was not a version message.
    BadVersion(VersionError),
    /// The key exchange did not complete.
    Handshake(handshake::HandshakeError),
    /// A frame was malformed.
    Framing(FrameError),
    /// A message's body was malformed.
    Message(messages::MessageError),
    /// The connection was idle for too long.
    Idle,
    /// The database failed.
    Database(asura_db::DatabaseError),
}

impl core::fmt::Display for ConnectionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ConnectionError::Io(err) => write!(f, "socket: {err}"),
            ConnectionError::UnknownConnection(err) => {
                write!(f, "no connection type in the first bytes: {err}")
            }
            ConnectionError::NotAnAuthConnection { kind } => {
                write!(f, "a {kind:?} connection does not belong on the auth port")
            }
            ConnectionError::BadVersion(err) => write!(f, "version: {err}"),
            ConnectionError::Handshake(err) => write!(f, "key exchange: {err}"),
            ConnectionError::Framing(err) => write!(f, "framing: {err}"),
            ConnectionError::Message(err) => write!(f, "message: {err}"),
            ConnectionError::Idle => write!(f, "connection was idle"),
            ConnectionError::Database(err) => write!(f, "database: {err}"),
        }
    }
}

impl std::error::Error for ConnectionError {}

/// One connection's state, after the key exchange.
struct Session {
    /// The connection's two scramblers, set up by the key exchange.
    established: handshake::Established,
    /// Decrypted bytes not yet consumed as frames.
    plaintext: Vec<u8>,
    /// The account this connection resolved to, once the identity pair was accepted.
    account: Option<Uuid>,
}

/// Whether to print the per-message trace.
///
/// The same shape as GWPS's `GWPS_TRACE`, and not optional in spirit: the client reports nothing but
/// `007`, so the trace is the only way to see what actually differed (`docs/03-protocol.md`, section 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trace {
    Off,
    On,
}

impl Trace {
    /// Read `ASURA_TRACE` from the environment: `1`, `true`, or `yes` turns it on. Anything else,
    /// including an unset variable, is off.
    pub fn from_environment() -> Self {
        match std::env::var("ASURA_TRACE") {
            Ok(value) if Trace::is_on_value(&value) => Trace::On,
            _ => Trace::Off,
        }
    }

    fn on(self) -> bool {
        matches!(self, Trace::On)
    }

    /// Whether a value means "on". Split out from [`Trace::from_environment`] so the rule can be
    /// tested without touching the process environment.
    pub fn is_on_value(value: &str) -> bool {
        let value = value.trim().to_ascii_lowercase();
        value == "1" || value == "true" || value == "yes"
    }
}

/// Accept connections until the listener fails.
///
/// Each connection is handled in its own task; a failure closes that connection and nothing else.
/// The service is restartable by design: it holds no state that a restart has to recover
/// (`docs/02-architecture.md`, "Startup order").
pub async fn serve(listener: TcpListener, pool: PgPool, trace: Trace) -> io::Result<()> {
    let address = listener.local_addr()?;
    eprintln!("asura-auth: listening on {address}");
    if trace.on() {
        eprintln!("asura-auth: per-message trace on (ASURA_TRACE)");
    }

    loop {
        let (stream, peer) = listener.accept().await?;
        let pool = pool.clone();
        tokio::spawn(async move {
            match handle_connection(stream, &pool, trace).await {
                Ok(()) => {
                    if trace.on() {
                        eprintln!("auth: {peer} closed");
                    }
                }
                Err(err) => eprintln!("auth: {peer}: {err}"),
            }
        });
    }
}

/// Drive one connection from the first byte to the close.
pub async fn handle_connection(
    mut stream: TcpStream,
    pool: &PgPool,
    trace: Trace,
) -> Result<(), ConnectionError> {
    let mut buffer = Vec::with_capacity(READ_CHUNK);

    // ---- 1. what kind of connection is this? ------------------------------------------------
    // Four bytes arrive first and are *kept*: they are the beginning of the version message, not a
    // message of their own.
    read_until(&mut stream, &mut buffer, 4).await?;
    let kind = version::classify(&buffer).map_err(ConnectionError::UnknownConnection)?;
    if kind != version::ConnectionKind::Auth {
        // A game or control connection belongs to a world server, not here. Refusing it is the whole
        // point of classifying before anything else.
        return Err(ConnectionError::NotAnAuthConnection { kind });
    }

    // ---- 2. the version message -------------------------------------------------------------
    read_until(&mut stream, &mut buffer, AUTH_CMSG_VERSION_LEN).await?;
    let (client_version, used) =
        version::AuthVersion::decode(&buffer).map_err(ConnectionError::BadVersion)?;
    buffer.drain(..used);
    if trace.on() {
        eprintln!(
            "auth <- version build={} h0008={} h000c={}",
            client_version.version, client_version.h0008, client_version.h000c
        );
    }

    // ---- 3. the key exchange ---------------------------------------------------------------
    read_until(&mut stream, &mut buffer, CLIENT_SEED_LEN).await?;
    let mut master_secret = [0u8; MASTER_SECRET_LEN];
    getrandom::getrandom(&mut master_secret)
        .map_err(|err| ConnectionError::Io(io::Error::other(err.to_string())))?;

    let established = handshake::accept(&buffer[..CLIENT_SEED_LEN], &master_secret)
        .map_err(ConnectionError::Handshake)?;
    buffer.drain(..CLIENT_SEED_LEN);

    if trace.on() {
        eprintln!("auth <- client seed ({} bytes)", CLIENT_SEED_LEN);
        eprintln!(
            "auth -> server seed ({} bytes), scrambled from here on",
            established.reply.len()
        );
    }
    stream
        .write_all(&established.reply)
        .await
        .map_err(ConnectionError::Io)?;

    // ---- 4/5. frames, then login ------------------------------------------------------------
    //
    // Note what happens when this connection ends: nothing is written to the database. A session is
    // *not* consumed by logging in — GWPS only ever reads its session table (`Db_GetSession`), and
    // the rig's portal hands out the same fixed pair on every login, so a service that closed the
    // session on disconnect would refuse the client's very next attempt. "The newest session wins"
    // is a rule about live *connections* and about leases (`docs/05-gw2-lessons.md`, rule 1); it is
    // not licence to spend a row.
    let mut session = Session {
        established,
        plaintext: buffer,
        account: None,
    };

    loop {
        match next_frame(&mut stream, &mut session, trace).await {
            Ok(Some(frame)) => {
                let keep_going = dispatch(&mut stream, &mut session, pool, frame, trace).await?;
                if !keep_going {
                    break;
                }
            }
            Ok(None) => break, // the client closed the connection
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

/// Handle one decrypted frame. `Ok(false)` means the connection is finished.
async fn dispatch(
    stream: &mut TcpStream,
    session: &mut Session,
    pool: &PgPool,
    frame: Frame,
    trace: Trace,
) -> Result<bool, ConnectionError> {
    if trace.on() {
        trace_frame("<-", &frame);
    }

    match frame.header {
        tables::AUTH_CMSG_PORTAL_ACCOUNT_LOGIN => {
            let login = PortalAccountLogin::decode(&frame).map_err(ConnectionError::Message)?;
            if trace.on() {
                eprintln!(
                    "auth: portal login request={} charname={:?} kind={:?}",
                    login.request, login.character_name, login.kind
                );
            }
            let replies = answer_login(pool, session, &login, trace).await?;
            send_frames(stream, session, replies, trace).await?;
            Ok(true)
        }

        // The settings uploads, which must be acknowledged: an unacknowledged one is a hang, and
        // GWPS lived through seven of those (`docs/03-protocol.md`, section 6). Answering every
        // request the client makes keeps that trap shut without having to understand the payload.
        tables::AUTH_CMSG_SETTING_UPDATE_SIZE | tables::AUTH_CMSG_SETTING_UPDATE_CONTENT => {
            let request =
                u32::from_le_bytes([frame.body[0], frame.body[1], frame.body[2], frame.body[3]]);
            let reply = RequestResponse {
                request,
                status: STATUS_OK,
            };
            send_frames(stream, session, vec![reply.frame()], trace).await?;
            Ok(true)
        }

        tables::AUTH_CMSG_HEARTBEAT => Ok(true),

        // Anything else is logged and skipped rather than closing the connection: the reference
        // warns and carries on, and dropping a connection over a message we simply do not use yet
        // would make the client's symptom worse than the cause.
        other => {
            eprintln!(
                "auth: unhandled header {other:#06X} ({} bytes)",
                frame.len()
            );
            Ok(true)
        }
    }
}

/// The login decision, expressed as the frames to send back.
async fn answer_login(
    pool: &PgPool,
    session: &mut Session,
    login: &PortalAccountLogin,
    trace: Trace,
) -> Result<Vec<Frame>, ConnectionError> {
    // The UUIDs travel in the little-endian RFC 4122 variant; reading them the ordinary way would
    // make every id wrong (`client::messages`, module documentation).
    let portal_user = Uuid::from_bytes_le(login.user_id);
    let session_id = Uuid::from_bytes_le(login.session_id);

    let resolved = db_login::find_session(pool, portal_user, session_id)
        .await
        .map_err(ConnectionError::Database)?;

    let Some(resolved) = resolved else {
        if trace.on() {
            eprintln!("auth: no session for {portal_user} / {session_id}; refusing");
        }
        // The client shows this as an auth error. The connection stays open: the reference answers
        // the request and waits, and so does this.
        session.account = None;
        return Ok(vec![RequestResponse {
            request: login.request,
            status: messages::STATUS_AUTH_ERROR,
        }
        .frame()]);
    };

    // The session table is read, never spent: GWPS's login path calls `Db_GetSession` and writes
    // nothing, and the rig's portal presents the same fixed pair on every login. Closing rows here
    // would refuse the client's next attempt. The "newest wins" rule applies to live connections and
    // to leases, not to this row (`docs/03-protocol.md` section 4).
    let characters = db_login::characters(pool, resolved.account)
        .await
        .map_err(ConnectionError::Database)?;

    session.account = Some(resolved.account);
    if resolved.character.is_some() && trace.on() {
        eprintln!(
            "auth: the session records character {:?} as last selected",
            resolved.character
        );
    }

    // The order GWPS sends, which the client expects (`AuthSrv_HandlePortalAccountLogin`): the
    // characters, then the friend-list terminator, then the answer to the request. The account
    // settings and account info messages that GWPS interleaves are the next piece of the port; the
    // character list is what this milestone needs.
    let mut replies = Vec::with_capacity(characters.len() + 2);
    for character in &characters {
        replies.push(
            messages::character_info(login.request, character.id.to_bytes_le(), &character.name)
                .map_err(ConnectionError::Message)?,
        );
    }
    replies.push(messages::friend_stream_end(login.request));
    replies.push(
        RequestResponse {
            request: login.request,
            status: STATUS_OK,
        }
        .frame(),
    );

    if trace.on() {
        eprintln!(
            "auth: login ok for {} with {} character(s)",
            resolved.account,
            characters.len()
        );
    }
    Ok(replies)
}

/// Scramble and write frames.
async fn send_frames(
    stream: &mut TcpStream,
    session: &mut Session,
    frames: Vec<Frame>,
    trace: Trace,
) -> Result<(), ConnectionError> {
    for frame in frames {
        if trace.on() {
            trace_frame("->", &frame);
        }
        let mut bytes = frame.encode_validated().map_err(ConnectionError::Framing)?;
        session.established.outgoing.apply(&mut bytes);
        stream
            .write_all(&bytes)
            .await
            .map_err(ConnectionError::Io)?;
    }
    Ok(())
}

/// The next frame, reading from the socket as needed. `Ok(None)` is a clean disconnect.
async fn next_frame(
    stream: &mut TcpStream,
    session: &mut Session,
    trace: Trace,
) -> Result<Option<Frame>, ConnectionError> {
    loop {
        match Frame::decode(&session.plaintext) {
            Ok((frame, used)) => {
                session.plaintext.drain(..used);
                return Ok(Some(frame));
            }
            Err(FrameError::Incomplete { .. }) => {}
            Err(err) => return Err(ConnectionError::Framing(err)),
        }

        let mut chunk = [0u8; READ_CHUNK];
        let read = match tokio::time::timeout(IDLE_TIMEOUT, stream.read(&mut chunk)).await {
            Err(_) => return Err(ConnectionError::Idle),
            Ok(Ok(0)) => return Ok(None),
            Ok(Ok(read)) => read,
            Ok(Err(err)) => return Err(ConnectionError::Io(err)),
        };

        let mut plaintext = chunk[..read].to_vec();
        session.established.incoming.apply(&mut plaintext);
        session.plaintext.extend_from_slice(&plaintext);
        if trace.on() {
            eprintln!("auth <- {read} byte(s) on the wire");
        }
    }
}

/// Read into `buffer` until it holds at least `wanted` bytes.
///
/// The buffer is the caller's, and it is *not* consumed here: the first four bytes of a connection
/// are the start of the version message, so discarding them after classifying the connection would
/// lose the message that was being classified.
async fn read_until(
    stream: &mut TcpStream,
    buffer: &mut Vec<u8>,
    wanted: usize,
) -> Result<(), ConnectionError> {
    while buffer.len() < wanted {
        let mut chunk = [0u8; READ_CHUNK];
        let read = match tokio::time::timeout(IDLE_TIMEOUT, stream.read(&mut chunk)).await {
            Err(_) => return Err(ConnectionError::Idle),
            Ok(Ok(0)) => {
                return Err(ConnectionError::Io(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "the client closed the connection mid-handshake",
                )))
            }
            Ok(Ok(read)) => read,
            Ok(Err(err)) => return Err(ConnectionError::Io(err)),
        };
        buffer.extend_from_slice(&chunk[..read]);
    }
    Ok(())
}

/// One trace line: direction, header, name, size and the first bytes (`docs/03-protocol.md` §6).
fn trace_frame(direction: &str, frame: &Frame) {
    let name = tables::entry(frame.header)
        .and_then(|entry| entry.name)
        .unwrap_or("unnamed");
    let preview: String = frame
        .body
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ");
    eprintln!(
        "auth {direction} {:#06X} {name} {} bytes: {preview}",
        frame.header,
        frame.len()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_trace_switch_accepts_what_the_reference_accepts() {
        assert!(Trace::is_on_value("1"));
        assert!(Trace::is_on_value("true"));
        assert!(Trace::is_on_value("TRUE"));
        assert!(Trace::is_on_value(" yes "));

        assert!(!Trace::is_on_value(""));
        assert!(!Trace::is_on_value("0"));
        assert!(!Trace::is_on_value("off"));
        assert!(!Trace::is_on_value("no"));
    }
}

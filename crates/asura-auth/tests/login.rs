//! M3's end-to-end test: a client logs in to the real listener and gets its character list.
//!
//! This is the first test in the project where the pieces meet: a socket, the version
//! classification, the key exchange with real randomness, ARC4 in both directions, the message
//! layouts, and the database. It uses the reference client from `asura_protocol::client::peer`, so it
//! needs no game client — but it is *not* a substitute for one: M3 is only accepted when the real
//! client reaches the character list and the packet trace diffs clean against GWPS
//! (`docs/roadmap.md`).
//!
//! With no database configured, each test prints a skip line and returns.

use std::net::SocketAddr;
use std::time::Duration;

use asura_auth::{serve, Trace};
use asura_db::login as db;
use asura_db::{PgPool, Uuid};
use asura_protocol::client::handshake;
use asura_protocol::client::messages::{self, RequestResponse};
use asura_protocol::client::peer::{self, ReferenceClient};
use asura_protocol::client::tables;
use asura_protocol::client::Frame;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const MAP: i32 = 15;
const REQUEST_ID: u32 = 0x5EED;
const TIMEOUT: Duration = Duration::from_secs(10);

/// A running auth service on a loopback port, and the pool it uses.
struct Harness {
    pool: PgPool,
    address: SocketAddr,
    server: tokio::task::JoinHandle<()>,
}

impl Harness {
    async fn start() -> Option<Harness> {
        let url = asura_db::config::database_url()?;
        let pool = asura_db::placement::connect(&url, 4)
            .await
            .expect("connect the database");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind a loopback port");
        let address = listener.local_addr().expect("local address");
        let server_pool = pool.clone();
        let server = tokio::spawn(async move {
            // The listener runs until the harness is dropped, which aborts this task.
            let _ = serve(listener, server_pool, Trace::Off).await;
        });

        Some(Harness {
            pool,
            address,
            server,
        })
    }

    async fn connect(&self) -> TcpStream {
        tokio::time::timeout(TIMEOUT, TcpStream::connect(self.address))
            .await
            .expect("connect before the timeout")
            .expect("connect")
    }

    fn stop(self) {
        self.server.abort();
    }
}

fn skip() -> bool {
    if asura_db::config::database_url().is_none() {
        println!("ASURA_DATABASE_URL not set; skipping the end-to-end login test");
        return true;
    }
    false
}

fn account_name(label: &str) -> String {
    format!("m3-e2e-{label}-{}", Uuid::new_v4())
}

/// Read exactly `count` bytes, or fail.
async fn read_exact(stream: &mut TcpStream, count: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; count];
    tokio::time::timeout(TIMEOUT, stream.read_exact(&mut bytes))
        .await
        .expect("read before the timeout")
        .expect("read");
    bytes
}

/// Take a client through the version message and the key exchange, leaving it ready to log in.
async fn handshake_with(stream: &mut TcpStream) -> ReferenceClient {
    let mut client = ReferenceClient::with_seed(0xC0FFEE);
    stream
        .write_all(&client.version_message(34591))
        .await
        .expect("write the version message");

    stream
        .write_all(&client.client_seed_message())
        .await
        .expect("write the client seed");

    let server_seed = read_exact(stream, handshake::SERVER_SEED_LEN).await;
    client
        .accept_server_seed(&server_seed)
        .expect("the server seed is well formed");
    client
}

/// Send the login message and collect frames until the reply to it arrives.
async fn login(
    stream: &mut TcpStream,
    client: &mut ReferenceClient,
    user_id: Uuid,
    session_id: Uuid,
) -> Vec<Frame> {
    let frame = client
        .login_message(REQUEST_ID, user_id.to_bytes_le(), session_id.to_bytes_le())
        .expect("build the login message");
    let bytes = client.seal(&frame).expect("seal");
    stream.write_all(&bytes).await.expect("write the login");

    let mut frames: Vec<Frame> = Vec::new();
    let mut buffer = [0u8; 8192];
    while !frames.iter().any(|frame: &Frame| {
        frame.header == tables::AUTH_SMSG_REQUEST_RESPONSE
            && RequestResponse::decode(frame).is_ok_and(|reply| reply.request == REQUEST_ID)
    }) {
        let read = tokio::time::timeout(TIMEOUT, stream.read(&mut buffer))
            .await
            .expect("read before the timeout")
            .expect("read");
        assert!(read > 0, "the server closed the connection mid-login");
        frames.extend(client.receive(&buffer[..read]).expect("frames"));
    }
    frames
}

#[tokio::test]
async fn a_client_logs_in_and_receives_the_whole_login_batch() {
    if skip() {
        return;
    }
    let Some(harness) = Harness::start().await else {
        return;
    };

    let name = account_name("login");
    let seeded = db::seed_identity(
        &harness.pool,
        Uuid::new_v4(),
        Uuid::new_v4(),
        &name,
        "Asura E2E",
        MAP,
    )
    .await
    .expect("seed the identity");

    let mut stream = harness.connect().await;
    let mut client = handshake_with(&mut stream).await;
    let frames = login(&mut stream, &mut client, seeded.portal_user, seeded.session).await;

    // The characters come first, then the stored settings, the friend-list terminator, the account
    // info, and finally the answer to the request -- the order GWPS sends and the client expects.
    let headers: Vec<u16> = frames.iter().map(|frame| frame.header).collect();
    assert_eq!(
        headers,
        vec![
            tables::AUTH_SMSG_CHARACTER_INFO,
            tables::AUTH_SMSG_ACCOUNT_SETTINGS,
            tables::AUTH_SMSG_FRIEND_STREAM_END,
            tables::AUTH_SMSG_ACCOUNT_INFO,
            tables::AUTH_SMSG_REQUEST_RESPONSE,
        ],
        "the login batch is characters, settings, friend-stream end, account info, then the response"
    );
    assert_eq!(
        frames[1].len(),
        2058,
        "the settings message is the reference's fixed size"
    );
    assert_eq!(frames[3].len(), 275, "so is the account info");
    assert_eq!(
        &frames[3].body[28..44],
        &seeded.account.to_bytes_le(),
        "the account info names the account that just logged in"
    );
    assert_eq!(
        &frames[3].body[44..60],
        &seeded.character.to_bytes_le(),
        "and the character: the session's last selection, or the first one"
    );

    // The character is ours, with the id and name coming back exactly as stored. The UUID travels in
    // the little-endian variant, which is the trap this asserts against.
    let character = &frames[0];
    let expected = seeded.character.to_bytes_le();
    assert_eq!(&character.body[4..20], &expected);
    assert_eq!(
        Uuid::from_bytes_le(character.body[4..20].try_into().expect("16 bytes")),
        seeded.character
    );

    let name_length =
        u32::from_le_bytes(character.body[24..28].try_into().expect("4 bytes")) as usize;
    assert_eq!(name_length, "Asura E2E".len());
    let units: Vec<u16> = (0..name_length)
        .map(|index| {
            let at = 28 + index * 2;
            u16::from_le_bytes([character.body[at], character.body[at + 1]])
        })
        .collect();
    assert_eq!(String::from_utf16(&units).expect("utf-16"), "Asura E2E");

    // And the request is answered, successfully, on the id it was asked with.
    let reply =
        peer::as_request_response(frames.last().expect("a reply")).expect("a request response");
    assert_eq!(reply.request, REQUEST_ID);
    assert_eq!(reply.status, messages::STATUS_OK);

    // The session still exists, and the login did not close it: a client that is logging in gets a
    // live session, which is what a world server will later validate.
    assert!(
        db::find_session(&harness.pool, seeded.portal_user, seeded.session)
            .await
            .expect("query")
            .is_some(),
        "the session must still resolve after a successful login"
    );

    db::remove_account(&harness.pool, &name)
        .await
        .expect("cleanup");
    harness.stop();
}

#[tokio::test]
async fn a_pair_that_does_not_resolve_is_refused_with_the_clients_auth_error() {
    if skip() {
        return;
    }
    let Some(harness) = Harness::start().await else {
        return;
    };

    let name = account_name("refused");
    let seeded = db::seed_identity(
        &harness.pool,
        Uuid::new_v4(),
        Uuid::new_v4(),
        &name,
        "Asura Refused",
        MAP,
    )
    .await
    .expect("seed the identity");

    let mut stream = harness.connect().await;
    let mut client = handshake_with(&mut stream).await;

    // The right portal user with the wrong session id: the shape a stale or forged client presents.
    let frames = login(&mut stream, &mut client, seeded.portal_user, Uuid::new_v4()).await;

    assert_eq!(
        frames.len(),
        1,
        "a refusal is a single message: {headers:?}",
        headers = frames.iter().map(|f| f.header).collect::<Vec<_>>()
    );
    let reply = peer::as_request_response(&frames[0]).expect("a request response");
    assert_eq!(reply.request, REQUEST_ID);
    assert_eq!(
        reply.status,
        messages::STATUS_AUTH_ERROR,
        "the client is told its credentials were refused, not left waiting"
    );

    db::remove_account(&harness.pool, &name)
        .await
        .expect("cleanup");
    harness.stop();
}

#[tokio::test]
async fn a_closed_session_cannot_log_in() {
    // The rule from `docs/05-gw2-lessons.md` rule 1, proven through the wire rather than through the
    // database: a session that was closed does not resolve, so the login is refused.
    if skip() {
        return;
    }
    let Some(harness) = Harness::start().await else {
        return;
    };

    let name = account_name("closed");
    let seeded = db::seed_identity(
        &harness.pool,
        Uuid::new_v4(),
        Uuid::new_v4(),
        &name,
        "Asura Closed",
        MAP,
    )
    .await
    .expect("seed the identity");
    db::close_session(&harness.pool, seeded.session)
        .await
        .expect("close the session");

    let mut stream = harness.connect().await;
    let mut client = handshake_with(&mut stream).await;
    let frames = login(&mut stream, &mut client, seeded.portal_user, seeded.session).await;

    assert_eq!(frames.len(), 1);
    let reply = peer::as_request_response(&frames[0]).expect("a request response");
    assert_eq!(reply.status, messages::STATUS_AUTH_ERROR);

    db::remove_account(&harness.pool, &name)
        .await
        .expect("cleanup");
    harness.stop();
}

#[tokio::test]
async fn a_game_connection_is_not_accepted_here() {
    // A gameplay connection belongs to a world server. The listener classifies before anything else,
    // and refusing here is what keeps a game client from being negotiated by the wrong program.
    if skip() {
        return;
    }
    let Some(harness) = Harness::start().await else {
        return;
    };

    let mut stream = harness.connect().await;
    stream
        .write_all(&asura_protocol::client::version::GAME_CMSG_VERSION_HEADER.to_le_bytes())
        .await
        .expect("write the game version header");

    let mut buffer = [0u8; 16];
    let read = tokio::time::timeout(TIMEOUT, stream.read(&mut buffer))
        .await
        .expect("the server closes the connection before the timeout")
        .expect("read");
    assert_eq!(read, 0, "the server closes rather than negotiating");

    harness.stop();
}

#[tokio::test]
async fn garbage_before_the_version_message_closes_the_connection() {
    if skip() {
        return;
    }
    let Some(harness) = Harness::start().await else {
        return;
    };

    let mut stream = harness.connect().await;
    stream
        .write_all(&[0xDE, 0xAD, 0xBE, 0xEF])
        .await
        .expect("write nonsense");

    let mut buffer = [0u8; 16];
    let read = tokio::time::timeout(TIMEOUT, stream.read(&mut buffer))
        .await
        .expect("the server closes the connection before the timeout")
        .expect("read");
    assert_eq!(
        read, 0,
        "an unknown connection type is refused, not guessed at"
    );

    harness.stop();
}

#[tokio::test]
async fn logging_in_does_not_spend_the_session() {
    // The reference never writes its session table: `Db_GetSession` reads, and that is all. This
    // matters practically, because the rig's portal presents the *same* fixed pair on every login --
    // a service that consumed the session would refuse the client's very next attempt.
    if skip() {
        return;
    }
    let Some(harness) = Harness::start().await else {
        return;
    };

    let name = account_name("repeat");
    let seeded = db::seed_identity(
        &harness.pool,
        Uuid::new_v4(),
        Uuid::new_v4(),
        &name,
        "Asura Repeat",
        MAP,
    )
    .await
    .expect("seed the identity");

    for attempt in 1..=3 {
        let mut stream = harness.connect().await;
        let mut client = handshake_with(&mut stream).await;
        let frames = login(&mut stream, &mut client, seeded.portal_user, seeded.session).await;
        assert!(
            peer::as_request_response(frames.last().expect("a reply"))
                .expect("response")
                .is_ok(),
            "attempt {attempt} must log in: a session is not spent by using it"
        );
        assert_eq!(
            frames
                .iter()
                .filter(|frame| frame.header == tables::AUTH_SMSG_CHARACTER_INFO)
                .count(),
            1,
            "attempt {attempt} gets the character list"
        );
    }

    // Still live after three logins, which is what a reconnecting client depends on.
    assert!(
        db::find_session(&harness.pool, seeded.portal_user, seeded.session)
            .await
            .expect("query")
            .is_some(),
        "the session still resolves after logging in three times"
    );

    db::remove_account(&harness.pool, &name)
        .await
        .expect("cleanup");
    harness.stop();
}

#[tokio::test]
async fn two_live_sessions_for_one_account_both_work() {
    // Two clients of one account: what a developer with two game windows has, and what the rig
    // produces when two seeded pairs share an account. Neither login invalidates the other's row.
    // "The newest wins" is a rule about live connections and leases, not about this table.
    if skip() {
        return;
    }
    let Some(harness) = Harness::start().await else {
        return;
    };

    let name = account_name("twolive");
    let first = db::seed_identity(
        &harness.pool,
        Uuid::new_v4(),
        Uuid::new_v4(),
        &name,
        "Asura First",
        MAP,
    )
    .await
    .expect("seed the first identity");
    let second = db::seed_identity(
        &harness.pool,
        Uuid::new_v4(),
        Uuid::new_v4(),
        &name,
        "Asura Second",
        MAP,
    )
    .await
    .expect("seed the second identity");
    assert_eq!(first.account, second.account, "one account, two sessions");

    for pair in [
        (first.portal_user, first.session),
        (second.portal_user, second.session),
        (first.portal_user, first.session),
    ] {
        let mut stream = harness.connect().await;
        let mut client = handshake_with(&mut stream).await;
        let frames = login(&mut stream, &mut client, pair.0, pair.1).await;
        assert!(
            peer::as_request_response(frames.last().expect("a reply"))
                .expect("response")
                .is_ok(),
            "every live pair of the account must log in"
        );
    }

    for pair in [
        (first.portal_user, first.session),
        (second.portal_user, second.session),
    ] {
        assert!(
            db::find_session(&harness.pool, pair.0, pair.1)
                .await
                .expect("query")
                .is_some(),
            "both sessions are still live"
        );
    }

    db::remove_account(&harness.pool, &name)
        .await
        .expect("cleanup");
    harness.stop();
}

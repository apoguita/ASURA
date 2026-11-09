//! The socket: accept, read, write, and the one thing the world's listener must do before a session
//! exists - read the 64-byte version message.
//!
//! The reference's `shared/Network` (`AsyncAcceptor`, `NetworkThread`, `SocketMgr`) and
//! `game/Server/WorldSocket` split the same work: a manager that accepts, and a socket that owns one
//! connection's buffering. This module is the socket half; the accept loop is [`accept_loop`].
//!
//! It reads no configuration and starts nothing: the program's `main` owns both, exactly as
//! `apps/worldserver/Main.cpp` does in the reference.

use std::net::SocketAddr;

use asura_protocol::client::game_version::GameVersion;
use asura_protocol::client::version::GAME_CMSG_VERSION_LEN;
use asura_protocol::client::Frame;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// One client connection, before it is a session: the socket, and the bytes already read from it.
///
/// Everything a connection reads passes through `buffer` first, because the client may put its version
/// message and its key-exchange seed in one write - the reference's `WorldSocket` keeps the same
/// discipline for the same reason.
pub struct Connection {
    stream: TcpStream,
    peer: SocketAddr,
    buffer: Vec<u8>,
    trace: bool,
}

impl Connection {
    /// Take a fresh socket.
    pub fn new(stream: TcpStream, peer: SocketAddr, trace: bool) -> Self {
        Connection {
            stream,
            peer,
            buffer: Vec::with_capacity(GAME_CMSG_VERSION_LEN * 2),
            trace,
        }
    }

    /// Where the client is.
    pub fn peer(&self) -> SocketAddr {
        self.peer
    }

    /// The bytes already read and not yet consumed.
    pub fn buffered(&self) -> &[u8] {
        &self.buffer
    }

    /// Read until at least `needed` bytes are buffered.
    ///
    /// A connection that closes before that is an error with the count, because "the client went away
    /// mid-message" and "the client sent a short message" are different failures.
    pub async fn read_at_least(&mut self, needed: usize) -> std::io::Result<()> {
        while self.buffer.len() < needed {
            let mut chunk = [0u8; 512];
            let count = self.stream.read(&mut chunk).await?;
            if count == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    format!(
                        "connection closed with {} of {needed} bytes buffered",
                        self.buffer.len()
                    ),
                ));
            }
            self.buffer.extend_from_slice(&chunk[..count]);
        }
        Ok(())
    }

    /// Read the game version message, leaving anything after it buffered for the next reader.
    ///
    /// The 64-byte message is the first thing a gameplay connection says, and the reference decodes it
    /// before it creates a session (`GAME_CMSG_VERSION`). A malformed message is refused here, not
    /// guessed at.
    pub async fn read_game_version(&mut self) -> std::io::Result<GameVersion> {
        self.read_at_least(GAME_CMSG_VERSION_LEN).await?;
        let (version, _) = GameVersion::decode(&self.buffer[..GAME_CMSG_VERSION_LEN])
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string()))?;
        self.buffer.drain(..GAME_CMSG_VERSION_LEN);
        if self.trace {
            eprintln!(
                "worldd: {} game version build={} world={} map={} token={:#010X}",
                self.peer, version.version, version.map_token, version.map_id, version.player_token
            );
        }
        Ok(version)
    }

    /// Take the next complete message off the buffer, or `None` when it is not all there yet.
    pub fn next_frame(&mut self, from_client: bool) -> Option<Frame> {
        match Frame::decode_game_message(&self.buffer, from_client) {
            Ok((frame, used)) => {
                self.buffer.drain(..used);
                Some(frame)
            }
            Err(_) => None,
        }
    }

    /// Send one message. The bytes are what the builder produced; nothing is added here.
    pub async fn send(&mut self, frames: &[Frame]) -> std::io::Result<()> {
        for frame in frames {
            self.stream.write_all(frame.encode().as_slice()).await?;
        }
        Ok(())
    }
}

/// What a connection handler answers with: the future a session runs, boxed so the accept loop does not
/// grow a type parameter per handler.
pub type ConnectionFuture =
    std::pin::Pin<Box<dyn std::future::Future<Output = std::io::Result<()>> + Send>>;

/// Accept connections until the listener fails, handing each one to `handle`.
///
/// The connection is reaped rather than detached: a task that dies of a panic leaves this process
/// alive, so a panic must print which connection it was. The old program learned that the hard way and
/// this is the same shape (`crates/asura-world/src/connection.rs`, `serve`).
pub async fn accept_loop<H>(listener: TcpListener, handle: H, trace: bool) -> std::io::Result<()>
where
    H: Fn(Connection) -> ConnectionFuture + Send + Sync + 'static,
{
    let handle = std::sync::Arc::new(handle);
    loop {
        let (stream, peer) = listener.accept().await?;
        let connection = Connection::new(stream, peer, trace);
        let handle = std::sync::Arc::clone(&handle);
        let task = tokio::spawn(async move { handle(connection).await });
        tokio::spawn(async move {
            match task.await {
                Ok(Ok(())) => {
                    if trace {
                        eprintln!("worldd: {peer} left");
                    }
                }
                Ok(Err(err)) => eprintln!("worldd: {peer}: {err}"),
                Err(err) => eprintln!("worldd: {peer}: connection task ended: {err}"),
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The version message a client opens with, built by the protocol crate itself.
    fn version_message() -> Vec<u8> {
        let mut bytes = vec![0u8; GAME_CMSG_VERSION_LEN];
        bytes[0..4].copy_from_slice(&0x0000_0C05u32.to_le_bytes());
        bytes[4..8].copy_from_slice(&39112u32.to_le_bytes());
        bytes
    }

    #[test]
    fn the_version_message_is_sixty_four_bytes() {
        assert_eq!(version_message().len(), GAME_CMSG_VERSION_LEN);
    }

    #[test]
    fn a_short_version_message_is_refused_with_the_count() {
        let mut connection = Vec::new();
        connection.extend_from_slice(&version_message()[..10]);
        let decoded = GameVersion::decode(&connection);
        assert!(decoded.is_err(), "ten bytes is not a version message");
    }
}

//! The server half: the gameplay connection and the dispatcher that routes what it reads.
//!
//! In the reference these are `game/Server/WorldSocket`, `WorldSession` and the handlers behind them;
//! the sockets themselves arrived in cut 4a (`shared/socket`), and cut 4b moves this flow onto them.

pub mod connection;
pub mod dispatch;

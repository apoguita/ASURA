# A.S.U.R.A. - Automated Server Utility for Remote Authentication

A private Guild Wars 1 server written in Rust, with PostgreSQL for persistent state.

## Start here

- [Working instructions](AGENTS.md)
- [Documentation index](docs/README.md): find guidance, operations, reference, research, history and planned work.
- [Project status](docs/work/project-status.md): features and their recorded acceptance evidence.
- [Latest session notes](docs/work/session-handoff.md)

## Run the server

```powershell
cargo run -p asura-supervisor -- start
```

[Running the server](docs/operations/running-the-server.md) covers setup, database prerequisites,
start/stop, logs and troubleshooting. [Linux setup](docs/operations/running-on-linux.md) records the
server-host procedure and its verification limits.

The game client runs on Windows. [Client patcher instructions](client-patcher/README.md) describe
preparing the executable and matching keys. [Recordings](recordings/README.md) describe the evidence archive.

## Understand the project

- [Architecture and component responsibilities](docs/guidance/architecture.md)
- [Client protocol](docs/reference/client-protocol.md)
- [Game packs](docs/guidance/game-packs.md) and [configuration directory](config/World/README.md)
- [Scripting and behavior](docs/guidance/scripting-and-behavior.md)
- [Project roadmap](docs/work/project-roadmap.md): planned work, with completed infrastructure work kept in history.

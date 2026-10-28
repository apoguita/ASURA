# Running ASURA

One page, in order: what you need once, how to start the stack, what success looks like, and what to do when it does not. Everything here is checked against the tree (`config/services.toml`, `crates/asura-supervisor`, the ports below); if a command here disagrees with the code, the code is right and this page is a bug.

## What you need, once

- **A checkout of this repository**, on Windows (where the game client runs) or Linux (a server host, `docs/17-running-on-linux.md`).
- **Rust with cargo** — the whole stack is built from it.
- **PostgreSQL, running.** The supervisor checks for it and waits, but never starts it: on Windows it is a service, on Linux systemd. "Managed elsewhere" is the boundary (`config/services.toml`).
- **A `.env` file**: copy `.env.example` to `.env` and fill in the password. `ASURA_DATABASE_URL` is the one mandatory value; the auth bind and the trace switch are optional.
- Optional: a real `Gw.dat`, only for regenerating the path-graph dump (`asura-world --dump-path-graphs`). The worlds serve the bundled `data/path-graphs.bin`, so nothing needs it to run.

## First time: the schema

```powershell
cargo run -p asura-supervisor -- schema
```

Applies the numbered migrations, in order, through `psql` (the same command on Windows and Linux). A fresh database needs this once; re-running it is harmless.

## Start the stack: one command

```powershell
cargo run -p asura-supervisor -- start
```

What that one command does, in order:

1. reads `config/services.toml` (what this machine runs, with arguments and log paths);
2. proves the database is reachable;
3. builds the services (fast when up to date);
4. seeds the dev identities and the map catalogue (the rows a fresh database needs);
5. starts, in order: web gate `127.0.0.1:6601`, auth `127.0.0.1:6112`, world `8001` on `6113`;
6. registers every service it started in the database's `service` table, and keeps the rows fresh;
7. watches them until Ctrl+C or `stop` — a crashed service is retried at most 5 times, 2 s apart, then reported FAILED (it never restarts forever).

Ctrl+C stops the services *through* the supervisor (they run in their own process group, so they never see the console's Ctrl+C themselves). Closing the console still ends everything attached to it; a hard kill of the supervisor (no Ctrl+C, no `stop`) orphans the services — there is no code that can run on SIGKILL.

## What success looks like

```powershell
cargo run -p asura-supervisor -- status
```

- the local table shows `webgate`, `auth` and `world-8001` as `ready`;
- the registry table shows a row for each, marked `fresh` (a row whose heartbeat is older than 60 s prints `STALE` with its age);
- the three ports above accept connections.

With the game client, success is: the client asks the web gate on `6601` who it is, sends its login to auth on `6112`, reaches character select, and entering the world sends it to `world-8001` on `6113`. `tools/run-client-login.ps1` automates that attempt and prints a verdict (LOGIN ACCEPTED / REFUSED / NO LOGIN ARRIVED).

The logs are one file per service: `logs/webgate.log`, `logs/auth.log`, `logs/world-8001.log`. They are quiet by default — events only; `ASURA_TRACE=1` (or `play.ps1 -Trace`) adds every packet.

## Everyday commands

```powershell
cargo run -p asura-supervisor -- status                # what is running, ready, restarting, or failed
cargo run -p asura-supervisor -- stop                  # stop everything (and the supervisor)
cargo run -p asura-supervisor -- stop --only NAME      # stop one service
cargo run -p asura-supervisor -- restart NAME          # stop and start one service
cargo run -p asura-supervisor -- build                 # build only
cargo run -p asura-supervisor -- schema                # apply the migrations
cargo run -p asura-supervisor -- seed                  # the dev identities and the map catalogue
cargo run -p asura-supervisor -- deploy-world --old 8001 --new 8002
```

Every command takes `--config FILE` (default `config/services.toml`). The control channel answers on `127.0.0.1:7711`; a second supervisor on the same machine is refused.

## Rolling deploy

Uncomment the `[services.world-8002]` block in `config/services.toml` (a second world on this machine needs its own `--bind`, `127.0.0.1:6114`), then:

```powershell
cargo run -p asura-supervisor -- deploy-world --old 8001 --new 8002
```

The new build starts and is proven registered **before** the old world is marked draining. From then on the newest build serves new arrivals and reconnects; the old world keeps the players already inside it, and leaves when its last player has disconnected. Nobody is kicked. The rules and the why: `docs/02-architecture.md` (Deployment and draining).

## The old scripts (they still work)

The supervisor is additive; the Windows scripts from before it keep working unchanged:

```powershell
.\tools\start-server.ps1      # database check -> web gate :6601 -> auth :6112 -> world :6113
.\tools\stop-server.ps1       # the same, backwards, and it says what is still listening
.\tools\m2-verify.ps1         # placement acceptance: integration tests + the fake-world scenario
```

`tools/test-battery.ps1` is the local "everything" command (it ends printing the real-client steps, never automating them). The testing policy is in `docs/HANDOFF.md`: the real client is the test; the suite is not a gate.

## The client

The game client is Windows-only and lives in the rig, not here (`docs/06-reference-session.md`). Launch it against the web gate, e.g. `-portal 127.0.0.1`, and log in as one of the seeded accounts (`user1@example.com` / `user2@example.com` / `user3@example.com`; the web gate does not check the password). What a good session looks like on the wire is in `recordings/` (`recordings/README.md`).

## Linux

`docs/17-running-on-linux.md`: the same supervisor commands; the client stays on Windows; the Linux host has not had a live run recorded yet, and the runbook says so. A world registered at a second address (the multi-machine proof) is `docs/18-multi-machine.md`.

## CI: format and lint, nothing else

`.github/workflows/ci.yml` runs two checks on every push:

- `cargo fmt --all -- --check` — the code is formatted;
- `cargo clippy --workspace --all-targets -- -D warnings` — the linter is clean.

Nothing else runs automatically. The test suite needs a live PostgreSQL database to mean anything, and the repository's testing policy says the game client is the real test and the suite is not a gate (`docs/HANDOFF.md`) — so CI is the cheap, database-free half, and the acceptance runs stay in the battery and with the real client. The decision is recorded in `docs/16-implementation-plan.md`, S7.

## When it goes wrong

| what you see | what it is | what to do |
|---|---|---|
| `cannot listen on 127.0.0.1:7711 ... another supervisor is probably running` | a supervisor is already up | `cargo run -p asura-supervisor -- stop`, then `start` |
| `could not bind 127.0.0.1:6113` | something else holds a port | `stop` waits for its ports to free; a leftover process needs killing once (`stop-server.ps1` says what is still listening) |
| the client shows error `007` | the server and client disagree on a message | `ASURA_TRACE=1`, then compare the trace with a good session in `recordings/` |
| registry rows say `STALE` | a service stopped publishing without leaving | `status` shows the state; the reaper frees orphaned pool leases; `stop`, then `start` |
| `ASURA_DATABASE_URL is not configured` | there is no `.env`, or it is missing the URL | copy `.env.example` to `.env` and fill it in |
| `cargo cannot reach crates.io` | no network, or a sandbox token blocking TLS | the stack runs offline once dependencies are cached; from a normal shell, or with network access allowed |

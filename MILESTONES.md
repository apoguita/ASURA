# Milestones

The ledger of what this server has been **proven** to do. Every entry names the evidence that proves
it, because the testing policy here is the real client (`docs/HANDOFF.md`): a feature is not "working"
until the game client does it or a test drives it over a real socket.

**How to use this page.** When something is proven, move its row and name the proof. When a proof
changes hands (a client session, a new test), update the evidence, not the claim. If a row is wrong,
the row is a bug.

| status | means | proof it needs |
|---|---|---|
| **Client** | the real game client (build 39112) did it | the session recording or document in the row |
| **Test** | an automated test drove real sockets or the live database | the test name in the row |
| **Built** | implemented, not yet confirmed by either | no proof yet — say so |
| **Open** | not implemented | — |

## The client

| milestone | status | evidence |
|---|---|---|
| A stock `Gw.exe` is patched into `Gw.custom.exe` | Test | `client-patcher/patch-gw.py`; rebuilding from the rig's `Gw.exe` reproduces the installed `Gw.custom.exe` byte for byte (SHA-256 match, 2026-10-28) |
| The patched client runs with the mutex check removed, side by side with another copy | Test | the mutex patch in `client-patcher/patch-gw.py` |
| The client asks the web gate who it is (four `/Spawned/WebGate` endpoints) | Client | `docs/03-protocol.md` §4; the gate answers `GET /Spawned/WebGate/session/create.xml` with a session UUID (`200`) |
| Login: key exchange, session, character list | Client | `docs/06-reference-session.md`; `recordings/asura/world/live-client-session-*.jsonl` |
| The client reaches character select and enters an outpost | Client | `docs/HANDOFF.md` (M4); the live-client recordings, build 39112 |

## A player and their character

| milestone | status | evidence |
|---|---|---|
| A character's position is saved when leaving and resumed on entering | Test | `crates/asura-db/tests/inventory.rs`; `asura_db::game::record_position` |
| The item block is built from rows, and drops, moves and equipment persist | Client | `docs/HANDOFF.md` § items; `recordings/` |
| Chat speaks on `!`, `#`, `$`; `/stuck` and `/maps` answer | Client | `docs/HANDOFF.md` (items on a live client) |
| A character is created (name, appearance, profession, starting equipment) | Test | `crates/asura-world/tests/load.rs` — `a_character_is_created_in_a_creation_instance`; `docs/07-status.md` § creation |
| A character is deleted by name, only within the owning account | Built | `crates/asura-auth/src/lib.rs` `AUTH_CMSG_DELETE_CHARACTER` → `asura_db::login::delete_character` — no test or client session yet |
| The client's character-selection message (`AUTH_CMSG_CHANGE_PLAY_CHARACTER`) | Open | `docs/HANDOFF.md`: logged as unhandled; the session already points at the character |

## Instances, maps and movement

| milestone | status | evidence |
|---|---|---|
| A world hosts several maps in one process | Client | `docs/03-protocol.md` (travel loop 148 → 163 → 164 → 165 → 166 → 148) |
| A travel between maps: request, redirect, reconnect into the destination spawn | Client | `docs/06-reference-session.md`; `recordings/asura/world/live-client-session-*.jsonl` |
| A destination is loaded on demand, with nothing prebuilt (0–2 ms per map) | Client | `docs/03-protocol.md` §5c, measured 2026-10-27; pinned by `a_travel_to_an_unhosted_map_is_loaded_on_demand`, `a_connection_to_an_unhosted_map_is_loaded_on_demand` |
| Click-to-walk answers with pathfound waypoints | Client + Test | walked on the live client; `crates/asura-world/src/pathfind.rs` tests; `crates/asura-world/tests/path_dump.rs` |
| `MOVE_TO_COORD` refuses and answers with the agent's position when no path exists | Test | the movement rules in `crates/asura-world/src/lib.rs` |
| The world's clock tick and the client's movement cadence | Client | `recordings/client/feature/movement/movement-sniff.log`; `docs/HANDOFF.md` |
| Two clients in one map see each other | Test | the peer tests in `crates/asura-world/tests/load.rs` |

## Services, placement and the front door

| milestone | status | evidence |
|---|---|---|
| The auth service answers login, keeps sessions, refuses a travel cleanly | Client | `docs/02-architecture.md`; `docs/06-reference-session.md` |
| The front door relays the game connection to the world it placed the travel on | Client | real client sessions; `crates/asura-auth/src/frontdoor.rs` |
| Placement reads the registry, heartbeats decide freshness, a stale world is not offered | Test | `crates/asura-db/tests/placement.rs` |
| Pool capacity refuses a full world without disturbing a lease | Test | `crates/asura-db/tests/placement.rs`; `tools/m2-verify.ps1` |
| The reaper frees leases a silent world leaves | Test | `crates/asura-db/tests/placement.rs` |
| The front door dials a world registered at a second address (`127.0.0.2`), not its bind address | Test | `docs/18-multi-machine.md` (front-door trace) — the real-client row is still open |
| The newest build is the only one offered; a full newest build refuses instead of falling back | Test | `the_newest_build_is_the_only_one_offered` in `crates/asura-db/tests/placement.rs` |

## Operations and delivery

| milestone | status | evidence |
|---|---|---|
| The supervisor starts, stops, restarts, builds, applies the schema and seeds the data | Test + live | `crates/asura-supervisor` tests; live runs in `docs/19-running.md` |
| Every service the supervisor starts appears in the `service` registry, fresh, and `stopped` after a stop | Test + live | the catalog test; `docs/19-running.md` |
| A rolling deploy runs a second world beside the first and drains the old one when it empties | Test + live | `deploy-world` live run (world 8001 drained, 8002 stayed); the real-client half is open |
| The stack runs as two containers (optional `docker/` bundle) | Test | `docs/19-running.md` § Docker — built, started, ports answered, graceful stop |
| CI runs format and lint on every push | Test | `.github/workflows/ci.yml` (green locally; the first GitHub run is the proof on the runner) |

## The evidence archive

`recordings/` is where the proofs live, one row per recording in `recordings/index.jsonl`
(`recordings/README.md` has the naming scheme: `source/kind/subject`, source = `asura` | `gwps` |
`client`, kind = `login` | `world` | `session` | `feature/<movement|items|travel|chat>`). A new
milestone that is proven with the client gets its recording there and the row here names it.

## Open at the bottom of the list

These are deliberately not claimed above; each has a row with `Open` or `Built` where it belongs:

- character deletion: implemented in the auth service, not yet proven.
- the client's character-selection message.
- two real-client rows: a player standing in an instance during a rolling deploy, and a travel to the
  second address (`docs/16-implementation-plan.md` S3/S6, `docs/18-multi-machine.md`).
- the server-side things the reference never wrote: picking a dropped item up, splitting a stack by
  hand, buying and selling, swapping two items in a move, `UNEQUIP_ITEM` (`docs/HANDOFF.md`).

#| D13 | `client::peer` (421 lines, a fake game client) is compiled only behind the `test-client` feature, which `asura-auth` and `asura-world` turn on in `[dev-dependencies]`; the server's production build carries no test client. `client/login_fixtures.rs` is **not** a fixture - it is the 591-byte `AUTH_SMSG_ACCOUNT_SETTINGS` payload that `messages::account_settings` sends on every login (`messages.rs:577`) - so it stays, and only its name is wrong | accepted | A test client in the production path is code that cannot fail in production and is never exercised there; a payload misnamed a fixture invites someone to delete it as dead test data | `crates/asura-protocol/Cargo.toml`, `crates/asura-protocol/src/client/mod.rs`, `crates/asura-auth/tests/login.rs:21`, `crates/asura-db` unused |
 Layer decisions: why the layers are arranged as they are

One entry per decision, with the reason and the evidence. This is the log the checklist in
`docs/37-rework-layers.md` points to: the checklist tracks what moved, this file tracks why the
rest was left where it is.

Status values: **accepted** (decided, do not re-open without a reason), **provisional** (decided
for now, the named condition would change it), **open** (not decided).

| # | decision | status | reason | evidence |
|---|---|---|---|---|
| D1 | A layer is a boundary one change can be made behind, and the project has sixteen of them | accepted | A change is made, tested and reverted per layer; a finer split makes each move unverifiable, a coarser one makes the blast radius unknown | `docs/37-rework-layers.md`, 89 files, 38,689 lines measured |
| D2 | A layer counts as covered only under six conditions: file is truth, every reader switched, old source deleted, equivalence diff empty, ascii plus touched tests pass, client tested where visible | accepted | A layer that "mostly moved" is two sources of truth, which is the defect being removed | `docs/37-rework-layers.md` |
| D3 | Content is JSON; `config/services.toml` stays TOML; YAML rejected | accepted | TOML has no null, and absence is meaningful in this content (`to_instance: Option<i32>`, `arrive: Option<(f32,f32)>`); the tooling already emits JSON; no crate has `serde_json` yet | `crates/asura-world/src/portals.rs:20-25`, `data/portals.json`, `crates/asura-supervisor/Cargo.toml` |
| D4 | The code is the truth, not a commit title or a document | accepted | `travel.rs:154-166` prints the `0x80C1` touch and returns; commit `8bcc029` is titled "the touch now starts the crossing". The code runs, the title does not | `crates/asura-world/src/travel.rs:154-166`, `git log` |
| D5 | Layer 1 (wire and codec) takes no configuration, ever | accepted | Its sizes and field orders are fixed by the game client; a config that could change one breaks the client, and editing one falsifies evidence | `docs/37-rework-layers.md` layer 1 |
| D6 | A file lives in the layer that owns the fact, not the layer that first consumed it | accepted | The four misplaced things in layer 1 are there because a generator or a test was written next to them | `pvp_items.rs`, `peer.rs`, `login_fixtures.rs` in `asura-protocol` |
| D7 | Layer 1's defect list is worked in this order: test client and fixtures first, `pvp_items.rs` once the content format exists, message-number consolidation last | provisional | Two moves are local (12 call sites); the number consolidation is 245 references over 111 constants in two crates, and its only safety net is "no number changes" | measured: 12 call sites in 4 test files; 245 references |
| D8 | Message numbers have one owner: `packets.rs`; `game/mod.rs`'s constants point at it instead of repeating it | accepted, execution deferred | Two copies of the same number are two answers to "what is message 0x013A" | 403 consts in `packets.rs`, 109 in `game/mod.rs` |
| D9 | The `map_portal` shape is the model for every content move: rows in one place with a `source` column per row, read once at start-up, the compiled table kept as fallback until the diff is empty | accepted | It is the only content move this tree has completed, and it is reversible | `crates/asura-db/migrations/0025_map_portals.sql`, `crates/asura-world/src/main.rs:199-222` |
| D10 | Layers that hold by design stay in Rust: the wire catalogue, the archive-derived map data, cartography, and the recordings | accepted | They are evidence or measured data, not settings; a file that could change them would be a way to lie about the client | `docs/37-rework-layers.md` rows 1, 11, 12, 16 |
| D11 | `game/pvp_items.rs` (1,567 lines, 387 entries, frozen, one reader) leaves the wire layer, but its new home is not chosen | open | Two candidates: a content file under `config/`, or an items module in the content layer. The generator that made it is retired, so the data cannot be regenerated either way | `crates/asura-protocol/src/game/pvp_items.rs:1-12`, `crates/asura-world/src/player.rs:162` |
| D12 | Whether a content file mirrors into the database, keys it, or owns content outright is undecided | open | Every content layer's target depends on it, so no content layer is ordered before it | `docs/37-rework-layers.md` layer 13 |

## How to add an entry

One row per decision, appended. Quote the reason in one sentence, name the evidence as `file:line`
or a measured count, and give it a status. A decision that changes is superseded by a new row that
names the old one - the old row is not edited.

# Layer rework checklist

Tracks the layer-by-layer move of configuration and content into `config/`. Measured from the code on
2026-11-03: 92 Rust files, 39,528 lines across 9 crates, 25 migrations, 35 `tools/` files.

File counts and task entries below retain that earlier inventory and its recorded evidence unless
explicitly updated. They are not a fresh inventory of the current tree; each open task needs its
current code checked before implementation.

The reason behind each decision is in `docs/reference/decisions/layer-decisions.md`.

This file is the tracker. It states no design of its own: every row names the files it covers,
and every box is ticked with the commit that completed that task. A layer is **covered** only when all six
conditions of the next section hold for it.

## What "covered" means, and it is six conditions

1. **The file is the source of truth.** The value lives in `config/` and nowhere else.
2. **Every reader reads the file.** All consumers listed on the row are switched.
3. **The old source is deleted.** The constant, the SQL `INSERT` or the compiled table is gone,
   in its own commit.
4. **The equivalence diff is empty.** The file reproduces the old source row for row, and the
   command that says so is named on the row.
5. **`.\tools\ascii-check.ps1` exits 0** and the tests the change touches pass.
6. **A real client was tested** wherever the layer is client-visible.

## The audit: why no layer is closed

Every layer's boxes were ticked from work that was done, and that turned out not to answer the question
that matters. Layer 1 read `[-]` holds by design while it carried 137 hand-copied instance pairs, two
file-check payloads, a 591-byte login payload and a default appearance - data, in a layer of codecs. A
ticked box says a task was done; it does not say the layer is clean.

So each layer is audited, once, by the four questions from `AGENTS.md` 4a, file by file:

1. Does the file hold **the client's vocabulary** - its own enum values, header numbers, payloads?
2. Does it hold **values that describe the world** - maps, spawns, defaults, cadences?
3. Does it hold **runtime state** - rows that change while the server runs?
4. Does it hold a **decision** - "when X then Y" over data the caller already passes in?

Anything that is none of those is ordinary code and stays. Anything that is (1) or (2) is a candidate for
a file; (3) belongs to the layer that owns the entity; (4) belongs to layer 3.

Beside the four questions, the audit asks one boundary question: does the file reach through another
layer - read that layer's tables, its files or its compiled constants directly, instead of calling
the function that owns them? A reach-through is a finding even when the value sits in the right kind
of place, because it makes two layers depend on one fact.

No layer is closed (owner, 2026-11-08). Layer 1 was audited on 2026-11-08 and its findings are
open; the other 17 remain `audit pending`. Completed task boxes and earlier audit findings remain as
evidence of that work, not layer acceptance.

## How a layer is audited

One layer per session. The audit reads the layer's files and writes only this file and, where a
finding's home is a decision, `docs/reference/decisions/layer-decisions.md`.

1. Refresh the file list. The counts in "The layers" were measured 2026-11-03; count what the layer
   holds now and record the new number in the row, with the date it was read.
2. Classify each file's contents against the four questions above and the boundary question, one item
   per line with its `file:line`.
3. Hunt the fallbacks. A fallback is any of these, and each one is a finding even when it matches the
   reference: a default written where the input is missing, a value compiled where a config file
   should hold it, a second code path that nothing calls, a captured answer used as an oracle, a
   field accepted without being compared, a table that carries no evidence.
4. Write the findings under `#### Findings, <date>` inside the layer's `### Layer <N> audit`
   subsection, in three groups with these exact headings, in this order: `##### Not the client's
   vocabulary`, `##### Fallbacks that could interfere later`, `##### What holds`. Every box in the
   first two groups names its `file:line`. The third group is the work checked and left alone, so the
   next reader does not redo it.
5. Set the layer's status word: `audited, findings open`.
6. File the moves after the audit, one commit per box, in D22's order for content: write the file,
   switch the readers, delete the old source last in its own commit.
7. Close the layer only by the six conditions above. A clean audit does not close a client-visible
   layer, which still waits for the client check.
8. A finding that needs the owner's choice is reported with the choice named, and its box stays
   open. The audit does not decide it.

The record is the enforcement. A finished pass leaves a record a person can check without reading the
code twice: three headings, every finding with its `file:line`, and a holds list that names what was
read and left alone. No script checks that shape, and none should: a check that could pass the file
would reward filling the headings in, while the value of a pass is what it found. The owner reads the
record and the code it names, and a heading with nothing under it is a defect one line of reading
catches.

A layer whose audit finds nothing still gets its section: the date, the three headings with "none
found" under the first two, and what was read under the third, so the next reader knows the pass
happened.

A pass is not finished, and the layer's status stays `audit pending`, when any of these is true:

- a file in the layer was not opened and read;
- a finding carries no `file:line`;
- a group is missing, or says "none found" while the layer holds a compiled value, a code path with
  no caller, or a captured answer the server does not send;
- a fix travelled in the same change as the audit.

## Notation

A layer's status column holds one of three words: `audit pending`, `audited, findings open`, and
`closed`. The middle word is a finished pass with findings still unfixed; only the six conditions
above turn a layer `closed`.

- `[ ]` open - not started
- `[~]` partial - some boxes below are ticked
- `[x]` task recorded as done - retain its evidence; it does not close the layer
- `[-]` holds by design - this layer is not configuration and must not move

## The layers

| # | layer | files (measured) | class | status |
|---|---|---|---|---|
| 1 | wire and codec | `asura-protocol`, 14 files, 8,415 lines measured 2026-11-08; the packet table and the codecs, no payload of its own | evidence | audited, findings open |
| 2 | storage and schema | `asura-db`, 16 files, 4,814 lines, 25 migrations | schema + state + content passing through | audit pending |
| 3 | rules | `asura-rules`, 1 file, 283 lines, 9 tests; depends on 1 for the client's `MapKinds` | rules | audit pending |
| 4 | auth service | `asura-auth`, 6 files, 2,664 lines; depends on 3 | rules | audit pending |
| 5 | socket layer | `asura-gateway`, 3 files, 534 lines; depends on 4 | transport | audit pending |
| 6 | web gate | `asura-webgate`, 2 files, 696 lines | rules | audit pending |
| 7 | sessions | `asura-world`: `connection.rs`, `dispatch.rs`, `world.rs`, `map.rs`, `catalogue.rs`; `capture.rs` and `replay.rs` behind `harness` | rules + rates | audit pending |
| 8 | instance and simulation | `instance.rs`, `agent.rs`, `player.rs` | rules + rates | audit pending |
| 9 | travel | `travel.rs`, `portals.rs`, `travel_targets.rs` | rules + content | audit pending |
| 10 | character | `creation.rs` | rules + content | audit pending |
| 11 | items | `item.rs`, `pvp_items.rs` | rules + content | audit pending |
| 12 | chat and commands | `chat.rs` | rules + one table | audit pending |
| 13 | map data and pathing | `map_data.rs`, `pathfind.rs`, `gwdat.rs`, `fa_compress.rs`, `fa_compress_tables.rs`, `path_tables.rs`, `graph_dump.rs` | measured data | audit pending |
| 14 | cartography | `cartography.rs` | measured data | audit pending |
| 15 | content itself | four homes, see layer 15 | content | audit pending |
| 16 | supervision and operations | `asura-supervisor`, 10 files, 2,070 lines, `config/services.toml` | operations | audit pending |
| 17 | harness | `asura-fakeworld` 307 lines, 12 test files, 5 examples | harness | audit pending |
| 18 | evidence | `recordings/`, `data/portals.json`, `client-patcher/`, `logs/` | evidence | audit pending |

## Layer 1 - wire and codec

Files: `packets.rs` 2,106 (782 rows, 403 consts), `game/mod.rs` 1,711, `game/appearance.rs` 68,
`game/instance_table.rs` 175, `client/frame.rs` 660, `client/messages.rs` 1,267,
`client/handshake.rs` 435, `client/reference_client.rs` 383 (gated), `client/version.rs` 275,
`client/game_version.rs` 201, `client/arc4.rs` 190, `client/filecheck.rs` 95, `client/mod.rs` 105,
`client/account_settings.rs` 49, `trace.rs` 52, `lib.rs` 181.

- [-] holds by design - the packet table is the specification (`AGENTS.md` #1)
- [x] content that was misplaced here is moved out: `game/pvp_items.rs` (387 rows) is
      `crates/asura-world/src/pvp_items.rs`, layer 11 - commit `fb8e508`, rows compared row for row
      with the commit before, 93 protocol and 61 world unit tests pass
- [x] test client gated: `client/peer.rs` behind the `test-client` feature (`docs/reference/decisions/layer-decisions.md`, D13) - commit
      `8e7448b`; `cargo check --workspace` and `--all-targets` exit 0, 93 protocol tests pass
- [x] Earlier payload identification: `client/login_fixtures.rs` was renamed during the naming pass
      below. Its subsequent move to configuration is recorded in the Layer 1 audit; it is not a
      current wire-crate file
- [x] header numbers have one source: `MAP_INIT_START` and `INSTANCE_TABLE` aliased `packets.rs`, the
      last two literals of 66 - commit `31b5bfa`, 93 protocol tests pass
- [x] the layer passes the repository's own checks: `cargo clippy -p asura-protocol --all-targets
      -- -D warnings` and `cargo fmt --check` are both clean for this crate - commits `f62ee4a`
      (two needless borrows) and `276a8c2` (rustfmt on `frame.rs`)
- [x] the workspace's format check is green: `cargo fmt --all --check` exits 0 - commit `1138762`,
      which formatted the nine drifted files in `asura-auth` and `asura-world` and one blank line in
      this crate; formatting only, 93 protocol, 61 world and 5 auth unit tests pass
- [x] the naming rule is written down in `client/mod.rs` (`docs/reference/decisions/layer-decisions.md`, D15) - commit `b2dbc2f`; the
      violations it exposes are the four boxes below, one commit each

### Layer 1 naming, in the order the rule exposes it

- [x] rename `client/login_fixtures.rs` to `client/account_settings.rs`: it is a 591-byte payload,
      not a fixture - 5 references, all inside `messages.rs` - commit `7a8112b`
- [x] rename `client/peer.rs` to `client/reference_client.rs`: it is a fake client, not a peer -
      54 references across four test files and two examples - commit `af8c24a` (which also
      rustfmt'd those files, taking them off the workspace's fmt list)
- [x] split the two `frame` functions in `client/messages.rs`: the fallible one is
      `PortalAccountLogin::try_to_frame`, the infallible `RequestResponse::frame` keeps its name -
      commit `1e450c8`, six call sites
- [x] `pack`/`unpack` and the `Appearance` they belong to are `game/appearance.rs`, not a `codec.rs`
      (D16) - commit `13c9e72`, three call sites unchanged through the re-export
- [x] `client/game.rs` is `client/game_version.rs`: two modules were called `game`, one per
      direction - commit `33666f8`
- [x] `reference_client::as_request_response` is `decode_request_response`: it reads a frame and can
      fail, which is what `decode_` means here - commit `acf64d8`
- [x] `character_update_factions` takes a `CharacterProgress` struct instead of thirteen positional
      dwords, so the `#[allow(clippy::too_many_arguments)]` is gone - commit `89ebfea`
- [x] the crate's only `#[allow(dead_code)]` is gone: `Reader::utf16` was unused - commit `c3c0552`
- [x] the lint policy is written and the crate opts in (`docs/reference/decisions/layer-decisions.md`, D17): `missing_docs` warn,
      `unsafe_code` deny, `clippy::all` warn, `broken_intra_doc_links` deny - commit `fb112aa`,
      which also documented the 70 items the policy exposed and allowed one `unsafe` (ARC4's
      zeroization) with its SAFETY reason

### Layer 1 audit

An earlier pass recorded the five findings that follow as addressed. The pass of 2026-11-08 read all
14 files and is recorded under "Findings, 2026-11-08"; the layer is `audited, findings open`, not
closed.

- [x] the instance table is data, not a codec: it moved to `crates/asura-world/src/instance_table.rs`
      (where it is sent), and `docs/reference/decisions/layer-decisions.md` D36 gives it its real home - constructed from the pack's map
      folders. That step landed: the pairs are walked out of `config/World/Vanilla/maps/`, 137 of them,
      and the wire crate holds none
- [x] the default appearance is a value: `config/World/Vanilla/character_defaults.json` (`appearance`),
      and `Appearance` is a pure codec again
- [x] the file check's two byte strings are the client's payload: `config/Server/file_check.json`, with
      the crate keeping only the comparison
- [x] the login batch's 591-byte blob is the client's payload: `config/Server/account_settings.json`,
      with the crate keeping only the framing
- [x] the cartography payload's size was the packet table's number written twice: it is derived from the
      row's `FieldKind::Array32, length: 64`, and a test holds the table and the frame together

#### Findings, 2026-11-08

The file list was refreshed first: 14 files and 8,415 lines today, against 16 files and 8,885 lines
measured 2026-11-03. Of the 133 constants in the crate, the ones below are the ones that are not a
header alias, a size, or a value the client itself defines (chat channels 3/11/12/7, agent property
ids 36/41/42/43, speed modes 1/9, agent types 1/4, `CHAR_CLASS_PLAYER_BASE`).

##### Not the client's vocabulary

- [x] A second protocol nothing uses, removed. `INTERNAL_MAGIC` (`"ASUR"`), `INTERNAL_VERSION` 1, the
     12-byte header and four `Kind`s had no caller outside this crate's own tests; a search over
      `crates/` found only the definitions and the tests. It is ours, not the client's, and
     `docs/guidance/architecture.md:51-52` says state and placement go through the database, not
     through messages - so it was an unused alternative path in the layer that is supposed to hold
     only the client's vocabulary. Commit `ed23a25`: `lib.rs` 198 lines to 34, 3 tests and 173 lines
     removed, the crate description no longer claims "internal control messages",
     `cargo test -p asura-protocol` 90 pass, `cargo check --workspace` exits 0, and a search for
     `INTERNAL_*` or `Kind::*` over every crate returns nothing.
- [ ] Two character stats compiled here: `PLAYER_HEALTH_MAX = 400` and `PLAYER_ENERGY_MAX = 25`
      (`crates/asura-protocol/src/game/mod.rs:356-358`), read by the world
      (`crates/asura-world/src/player.rs:344-349`). No config file holds them;
      `config/World/Vanilla/character_defaults.json` carries level, gold, appearance and secondary
      profession only. Under `AGENTS.md` 4a these are values describing the world.
- [ ] Two account facts compiled in a builder: the campaign mask `H0016` and the feature block whose
      third `u16` is the character-slot count (`crates/asura-protocol/src/client/messages.rs:600-605`).
      The call site passes request, account, character and EULA only
      (`crates/asura-auth/src/lib.rs:722-727`), so which campaigns the account owns and how many
      characters it may hold are decided in the wire layer.
- [ ] The server's private exponent is compiled: `SERVER_PRIVATE_DECIMAL`
      (`crates/asura-protocol/src/client/handshake.rs:57`, prime at `:53`). The module's argument is
      recorded there - the patched client carries the matching public value, so it is a protocol
      constant rather than a secret - and so is its consequence: one key for every deployment, and a
      per-machine key has no file to live in. Report only; the owner decides whether that is intended.

##### Fallbacks that could interfere later

- [ ] Missing input is written as zeros, silently. `character.unwrap_or([0u8; 16])` and the five zero
      bytes of an unused piece slot (`crates/asura-protocol/src/client/messages.rs:614`, `:481`).
      Both match the reference, and both make "no character" and "the zero uuid" the same bytes on
      the wire, so a caller's mistake has no shape of its own.
- [ ] A recorded reference answer sits in the crate as an oracle: `CHARACTER_SETTINGS_EXTENDED`
      (`crates/asura-protocol/src/client/messages.rs:385`), 62 bytes for one warrior standing in map
      146. It is the test that `character_settings` packs correctly, not a value the server sends;
      nothing may read it as server settings.
- [ ] Unvalidated handshake fields: `h0008` and `h0038` are observed and never compared
      (`crates/asura-protocol/src/client/version.rs:151`,
      `crates/asura-protocol/src/client/game_version.rs:42`, `:55`), as in the reference. A client
      build that starts comparing them gets no mismatch signal from this service.
- [ ] The table cannot carry evidence row by row: 742 of 784 rows have an empty evidence list and 391
      have no name (`crates/asura-protocol/src/packets.rs:233`, where `PACKETS` begins; the evidence
      field is `:215`). The naming queue owns this, not this layer's placement, but the
      `[-] holds by design` claim rests on the rows that do have evidence.

##### What holds

- The file check's bytes are configuration (`crates/asura-protocol/src/client/filecheck.rs:9-28`);
  the crate keeps the comparison and takes the question as a parameter.
- `Appearance` packs and unpacks only; its default is `config/World/Vanilla/character_defaults.json`
  (`crates/asura-protocol/src/game/appearance.rs:14-16`).
- The instance table is not in this crate, and no map pair is compiled here.
- Framing refuses rather than guesses: an unknown header is an error
  (`crates/asura-protocol/src/client/frame.rs:213`), and a client message is walked by its counted
  fields rather than the table's maximum (`:227-253`).
- `Frame::zeros` is test-only; every use sits in a `#[cfg(test)]` module.
- The test client is feature-gated (`crates/asura-protocol/src/client/mod.rs:49-51`, `:66-67`).
- `ASURA_TRACE` (`crates/asura-protocol/src/trace.rs:23`) is the crate's only environment read, and it
  is a debug switch rather than a value.

## Layer 2 - storage and schema

Files: `migrations.rs` 240, `config.rs` 116, `game.rs` 376, `inventory.rs` 946,
`inventory/seed.rs` 122, `placement.rs` 609, `login.rs` 245, `account.rs` 44, `catalog.rs` 110,
`portal.rs` 108, `lib.rs` 300, plus `migrations/0001`-`0026`.

- [x] physical schema stays SQL: 26 migrations, 14 tables (`docs/reference/decisions/layer-decisions.md`, D21)
- [x] content rows counted and named: 214 `INSERT`s in `0013`, `0014`, `0016`-`0022`, plus
      `map_config`, `map_spawn` and `map_portal` (`0025`)
- [x] the split is written at the source (`crates/asura-db/src/lib.rs`): which tables are operational
      state and which are content passing through, with the one-sentence test for a new table - commit
      `49681c2`
- [x] no lint suppression left in the crate: `insert_row` takes an `ItemColumns` instead of fifteen
      arguments, and `settings` reads a named `CharacterRow` instead of an eight-wide tuple - commit
      `d3afedb`, 21 unit tests pass
- [x] the crate adopts the lint policy (`docs/reference/decisions/layer-decisions.md`, D17): `missing_docs`, `unsafe_code` deny and
      `broken_intra_doc_links` cost 106 one-line docs and three doc links - commit `9925621`; clippy
      `-D warnings`, `cargo doc` and 21 unit tests pass
- [ ] Review remaining legacy content tables and inserts before removal. Current map readers
      already use the pack; see [World pack files](../../config/World/README.md). Reader migration
      does not establish that legacy SQL, helpers and tests are ready to delete.
- [x] `map_portal` is content with provenance (`0025_map_portals.sql`, a `source` column per row)
- [~] The `t_` rename migration exists (`crates/asura-db/migrations/0026_t_prefix.sql`, D31).
      The earlier entry claimed all queries/tools and workspace tests passed but named only
      `T_PREFIX_COMMIT`, a placeholder rather than a commit. Recover that evidence and check
      remaining consumers before treating the rename audit as complete
- [ ] No content reader survives without a caller. `crates/asura-db/src/game.rs` still defines three:
      `map_config` (`:60`, `:103`), `spawn_points` (`:73`) and `map_portals` (`:88`). Nothing under
      `crates/` calls them as of this entry. Delete them, then let D22's order drop the tables.
- [ ] A startup does not require a content table. `REQUIRED_TABLES` lists `t_map_config`
      (`crates/asura-db/src/lib.rs:151`); the required list loses it when the last reader goes.
- [ ] The seeded content rows leave with the pack. `0021_measured_maps_and_spawns.sql` carries 100
      `INSERT` statements and `0022_tour3_maps.sql` carries 115. They stay as stale rows until the
      deletion migration, and the deletion is the same subject as the readers above.
- [ ] The development seed states no content. `tools/seed-dev.sql` seeds three accounts, their
      sessions and characters; `map_id 146` is a placeholder its own comment names (`:11-12`). A
      placeholder map in the seed is a content fact in the operational table, so it is a finding,
      not configuration.

## Layer 3 - rules (`asura-rules`)

Files: `src/lib.rs` 283 lines, 9 tests, one dependency (`asura-protocol`, for the client's
`MapKinds` type).

- [x] created because a protocol answer needed a game rule and no existing crate fits: `asura-db` is
      storage and `asura-world` may not exist yet at login (`docs/reference/decisions/layer-decisions.md`, D25) - commit `4b09d17`
- [x] the rule holds no value of its own (`docs/reference/decisions/layer-decisions.md`, D34): `DEFAULT_OUTPOST` is gone. The fallback
      outpost is `config/Server/defaults.json` (`default_outpost`, read by `asura-config`), it has no
      built-in default in code - the key is required and `asura-gateway` refuses to start without the
      file - and it
      arrives at `placement` as an argument, which answers with it plus
      `Substituted::NothingToPlace` when the character's row names neither a map nor an outpost. The
      outpost a character enters is written by character creation, by a scripted event and by
      travelling - never by this layer. 10 rule, 5 config and 3 auth unit tests pass
- [x] the blank-name rule is here rather than in the world: `name_is_acceptable(&str)`, called by the
      creation confirm in place of its own `trim().is_empty()`. The error code it answers with stays
      where the frame is built, because a code is protocol. 9 rule and 64 world unit tests pass
- [-] the home for the next rule, and this **holds**: a rule that a protocol answer needs belongs here
      rather than in a service. Party and instance policy (`docs/work/world-simulation-plan.md` phase 2) is the named candidate
      in the earlier task record. This box records the intended home, not completion of the layer
      or a current audit of party and instance policy

- [x] Placement values reach the rule as arguments (`crates/asura-rules/src/lib.rs`, `placement`).
      [Server settings](../../config/Server/README.md) documents the files and fields; `immutable`
      applies to the client's own numbers, not every placement value (`AGENTS.md` 4a)

## Layer 4 - auth service (`asura-auth`)

Files: `src/lib.rs` 845 (the login conversation), `src/travel.rs` 291 (the placement mechanism: registry
lookup, seat, transfer row), 2 examples. No `main.rs` and no listener - the socket layer owns those, and
the integration tests live in `crates/asura-gateway/tests/`.

- [x] the game rules leave the service: last outpost, the creation exemption and the default outpost
      are `asura-rules::placement`, a pure function with eight tests (`docs/reference/decisions/layer-decisions.md`, D25) - commit
      `4b09d17`; `travel.rs` keeps the registry lookup, the seat and the transfer row
- [x] Earlier constant ownership was superseded by file-backed arguments. Auth reads defaults and
      passes the map kinds to `asura-rules::placement` (`crates/asura-auth/src/travel.rs:143-175`);
      see Layer 3 above. Commit `4b09d17` records the earlier split, not the current value source
- [x] the crate adopts the lint policy (`docs/reference/decisions/layer-decisions.md`, D17): seven needless borrows and one field doc -
      commit `4b09d17`; clippy `-D warnings` and `cargo doc` pass
- [x] no lint suppression, no dead code: the three error variants only classification and routing could
      raise left with those jobs - commit `099b0f0`
- [x] the relay left auth: the socket layer is `crates/asura-gateway` - listener, file check,
      classification, relay (`docs/reference/decisions/layer-decisions.md`, D26) - commit `be99de2`; auth is a library call from it and
      has no `main.rs`
- [x] the settings blob carried the wrong column for the client: `last_outpost` now sends `characters.map_id` (D28) - commit `836cdfa`; the login batch test against the live database proves the 62 bytes
- [x] the supervisor starts `asura-gateway` on the auth port (`config/services.toml`,
      `[services.auth]`) - commit `be99de2`, so a client sees no change
- [x] `docs/reference/glossary.md`, `docs/guidance/architecture.md`, `docs/history/service-supervisor-research.md`, `docs/operations/running-on-linux.md`, `docs/operations/running-the-server.md` and `AGENTS.md` name the two new layers and the process that listens - commit `55f9b5a`

The service dependency order is described in [Architecture](../guidance/architecture.md#components).
`asura-rules` also depends on `asura-protocol` for `MapKinds` (`crates/asura-rules/Cargo.toml`);
the earlier "no dependencies" description is obsolete.
## Layer 5 - socket layer (`asura-gateway`)

Files: `src/lib.rs` 230, `src/relay.rs` 174, `src/main.rs` 130. It listens on the address a client
dials, answers the file check, reads the four classification bytes and hands the socket on
(`docs/reference/decisions/layer-decisions.md`, D26).

- [x] created and wired: `asura-auth` has no listener and no `main.rs`, and `config/services.toml`
      `[services.auth]` starts `asura-gateway` - commit `be99de2`, so a client sees no change
- [x] the crate adopts the lint policy (`docs/reference/decisions/layer-decisions.md`, D17) and `cargo doc` builds - commit `be99de2`
- [x] the relay is named for what it does: `src/relay.rs`, not `frontdoor.rs`, which named the part after the whole crate - commit `46d0c82`
- [x] the file check is answered here rather than by the auth conversation, and the sequence is
      measured: the client's 21-byte request is the first thing on that socket, its failure is fatal to
      the login, and a server of ours has answered one on loopback (`docs/reference/client-protocol.md` 7, the two runs
      of 2026-10-25; the code records it at `crates/asura-gateway/src/lib.rs`, step 0) - commit `fca403c`
- [-] the seven words in that answer are refreshed per client build, which is maintenance rather than work
      on this layer: the procedure and the record are `docs/operations/running-the-server.md` ("Refreshing the file check's
      seven words"), the decision is `docs/reference/decisions/layer-decisions.md` D30, and what is not measured - the cost of a stale set -
      is stated in `docs/reference/client-protocol.md` 7; the layer remains audit pending
- [x] The earlier freshness-window extraction is recorded by commit `801abfe` (10 rules, 4 gateway
      and 3 auth unit tests recorded then). Current readers use `asura-config`, not a rules helper:
      `crates/asura-auth/src/travel.rs:48-52` and `crates/asura-gateway/src/relay.rs:152-158`
- [x] Layer description and correspondence recorded by commit `801abfe`: see
      [Architecture](../guidance/architecture.md) and
      [ADR 0006](../reference/decisions/0006-structural-design-authority.md#correspondence-between-the-two-systems)

## Layer 6 - web gate

Files: `lib.rs` 581, `main.rs` 115. The portal web gate: the four HTTP answers the
client takes its identity from before it logs in (`docs/reference/client-protocol.md` #4), plus `/health`.

- [x] the four answers checked and they hold no content: every body is a fixed reference string or
      a database lookup - the session id is minted, `UserId` and `Token` come from `asura_db::portal`,
      and the rest (`:John.10`, the `ResumeToken` placeholder, `Alias` `gw1`, `Created` 2005-01-01) is
      copied from the reference's `webgate.py:90-92` and belongs to the wire, like a field layout. The
      crate reads no file at all: `lib.rs` has no `std::fs` and no `include_str`, and its only settings
      are the bind address and the database URL (`main.rs:39,51`). The account list is rows, seeded by
      `tools/seed-dev.sql:15` - operational state under `docs/reference/decisions/layer-decisions.md`, D21 - checked against `lib.rs` 130-196
- [x] `/health` recorded as operational: a `GET` that answers `ok` so the supervisor can ask "are you
      alive", added by this project and not one of the reference's routes (`lib.rs:14-15,190-193`, and
      the route table says so again). Not content, and nothing reads it but supervision - checked against
      `main.rs:39,51` and `config/services.toml` (`[services.webgate]`, `health = { tcp = ... }`)

## Layer 7 - sessions

A **session** is one client's stay in the world: the socket arrives, the handshake runs, the ticket
the login issued is claimed, the load goes out, and then the connection is the world's until it
closes. It is not the login session, which is a row - `t_session` in the database, written by the
auth service and claimed here - and it is not the simulation, which is layer 8.

Files: `connection.rs` 572, `dispatch.rs` 210, `world.rs` 89, `map.rs` 137, `catalogue.rs` 103, plus
`capture.rs` and `replay.rs` behind the `harness` feature. The path graphs are `pathfind.rs`, the
spawn handler `agent.rs` and the player block `player.rs` (commit `8890603`).

- [x] the two instruments left the serving build: `capture.rs` and `replay.rs` are behind the
      `harness` feature, off by default, and `harness_off.rs` gives both modules an inert answer
      with the same shape so no call site reads a `cfg` (`docs/reference/decisions/layer-decisions.md`, D32). 55 tests pass in the
      serving build and 61 with `--features harness`, which is where the six instrument tests live
      - `crates/asura-world/src/harness_off.rs`
- [x] `map.rs` split, and what it never owned left: the path graphs are `pathfind.rs`, the spawn
      handler `agent.rs`, the catalogue entry `catalogue.rs`, the player block and the unlock facts
      `player.rs`. Current instance-table content is world-owned (Layer 1 audit), and cartography
      encoding is `cartography.rs`'s. `map.rs` builds the load and manifest and calls those helpers;
      it is not a separate implementation of their content. The earlier split is recorded by
      commits `8890603` and `54cb1f2`; load order is documented in `docs/reference/client-protocol.md` 11.3a
- [ ] Review legacy map-content cleanup separately from the implemented pack reader;
      see [World pack files](../../config/World/README.md).
- [x] the two timings and the ping value left `connection.rs`: the world reads them from
      `config/Server/intervals.json` through `asura-config` (`docs/reference/decisions/layer-decisions.md`, D33), and the file marks the
      tick as measured, the ping interval as the reference's constant and the ping value as
      unverified. The same file answers the login path's probe timeout and the freshness window the
      socket layer routes by, so the three programs cannot disagree - 4 config, 55 world, 3 auth,
      4 gateway and 10 rules unit tests pass
- [x] the load block's message order is recorded with its capture named:
      `docs/reference/client-protocol.md` 11.3a lists the six messages in the order this server sends them, the
      two that join afterwards, and the 24-message head from
      `recordings/asura/world/live-client-session-20261027-222641.jsonl` that proves the order a
      live client survives
- [x] a real client was run against this build after the moves (owner's session, 2026-11-04): login,
      load and play. The relocations changed no message, and this is the run that says so

## Layer 8 - instance and simulation

Files: `instance.rs` 378, `agent.rs` 1,158, `player.rs` 305.

- [~] Tick reader is wired: `connection::tick_interval` reads `asura-config` and `Instance::run`
      uses it (`crates/asura-world/src/connection.rs:23-29`, `instance.rs:346`). Current movement
      acceptance is in [Project status](project-status.md#maps-and-movement); the layer remains open
- [ ] `POSITION_UPDATE_MAX_ELAPSED_MS = 2000` moved to the rates file
- [ ] `EXPERIENCE_TEST_VALUE = 500` recorded as a stand-in, not moved
- [ ] `REACH`-style unmeasured constants listed in one place

## Layer 9 - travel

Files: `travel.rs` 522, `portals.rs` 232, `travel_targets.rs` 164.

- [~] Pack portal reader and removal of the compiled `CROSSED`/`PORTALS` fallback are implemented
      (`crates/asura-world/src/portals.rs`, `table_pack`, `crossed`, `arrival`). Current content
      loading is maintained in [World pack files](../../config/World/README.md); client verification
      of the later crossing repair remains pending in [Project status](project-status.md#pending-client-verification)
- [ ] `travel_targets.rs`'s 21 aliases over 12 maps moved to `config/` as per-map aliases
- [ ] `hosted` removed from content and computed at runtime
- [ ] the doorway identity question recorded: position (today) versus the client's prop index
- [~] `0x80C1` agent-touch travel is still unimplemented: `travel::handle_travel_init`
      (`crates/asura-world/src/travel.rs:140-161`) decodes and logs the touch, then returns without
      travel. That fact does not accept the behavior or settle the intended agent-touch policy

## Layer 10 - character

Files: `creation.rs` 482.

- [ ] `default_equipment.rs` (2,957 lines; 32 campaign rows, 179 items) moved to `config/`, or kept
      as code with the reason written down
- [~] Starter-map reader is implemented; campaign-by-campaign client verification remains open.
      See [Project status](project-status.md#remaining-feature-and-evidence-gaps) and
      [World pack files](../../config/World/README.md) instead of repeating its values here

## Layer 11 - items

Files: `item.rs` 525, `pvp_items.rs` 1,571.

- [x] `pvp_items.rs` arrived here from the wire layer: 387 unlockable-PvP-item rows, frozen, one
      reader (`player.rs:162`) - commit `fb8e508`
- [ ] `pvp_items.rs` decided: a `config/` file or a Rust table kept as evidence (D11, after D12)
- [ ] `inventory/seed.rs` (122 lines) moved to `config/`
- [ ] the bag order and the refusal shapes recorded as rules (they are not content)

## Layer 12 - chat and commands

Files: `chat.rs` 349.

- [ ] the `COMMANDS` table (`chat.rs:141`) moved to `config/`, or kept as code with the reason
- [ ] the coded-string chat body recorded as a wire rule, not content

## Layer 13 - map data and pathing

- [-] holds by design - measured from the archive, not authored
- [ ] Record build-artifact provenance for `data/path-graphs.bin`; the earlier inventory size
      predates the Northlands graph replacement described in [Session handoff](session-handoff.md#latest-gameplay-session-notes)
- [ ] how a profile ships it recorded (it is not `config/`)

## Layer 14 - cartography

- [-] holds by design, and it is a **frame without a feature** (`docs/reference/decisions/layer-decisions.md` D37): the grid's dimensions
      (64 x 128 for Tyria) and the band format are measured, and every band the world sends is empty
      because a character's exploration progress has no schema here. The bytes are therefore correct
      for a fresh character and wrong for everyone else - not a default chosen for convenience, an
      unimplemented feature
- [ ] the exploration bits exist: a table of explored cells per character per continent, the encoder
      for the client's band format in both directions, and an update path as the character walks. Until
      then every load announces a grid nobody has explored

## Layer 15 - content itself

Remaining compiled content and earlier inventories:

- [x] Historical SQL inventory: `map_config`, `map_spawn`, `map_portal`; current loading is
      documented in [World pack files](../../config/World/README.md).
- [ ] Review compiled Rust content in `default_equipment.rs`, `pvp_items.rs`, `inventory/seed.rs`
      and `travel_targets.rs`. Portal tables are no longer a compiled fallback (Layer 9)
- [ ] Review `data/portals.json` as research output: `tools/gen-portal-table.py` writes it;
      serving portal readers use the pack files described in Layer 9
- [ ] 214 `INSERT`s inside migrations `0013`-`0022`
- [~] Instance-table reader already moved to `crates/asura-world/src/instance_table.rs` and reads
      pack records (Layer 1 audit). The earlier move task is superseded, not still unstarted
- [~] File layout and `config/` readers exist; see [World pack files](../../config/World/README.md)
      and [Server settings](../../config/Server/README.md). Their existence does not close the content audit

## Layer 16 - supervision and operations

Files: `config.rs` 347, `supervise.rs` 571, `main.rs` 493, `control.rs` 128, `registry.rs` 114,
`psql.rs` 70, `process.rs` 68, `status.rs` 34, `health.rs` 22, `tests/supervisor.rs` 223,
`config/services.toml`, `config/services.docker.toml`, `docker/`.

- [x] already file-native: one TOML file drives every service (`config.rs:150`, `toml::from_str`)
- [ ] `deploy-world` covered by a test (it has none)

## Layer 17 - harness

Earlier file counts: `asura-fakeworld/src/main.rs` 307, `asura-world/tests/load.rs` 3,039,
`asura-world/tests/end_to_end.rs` 344, login/travel tests now in `asura-gateway/tests/`,
`asura-db/tests/*` (5 files, 1,483), `asura-world/tests/path_dump.rs` 220,
`asura-world/tests/portals.rs` 41, 5 examples.

- [ ] Recount current declared/ignored tests with their reasons before a harness audit; 261 tests
      and 2 ignored describe the earlier inventory, not current coverage or a new test run
- [ ] every fixture that is content moved with its layer

## Layer 18 - evidence

- [-] holds by design - never configuration, never edited

## Boundary defects to fix before the content layers move

- [ ] Audit the remaining dependency boundaries without assuming auth is unused: it is a
      dev-dependency (`crates/asura-world/Cargo.toml:30-34`) used for `Trace` in
      `crates/asura-world/tests/end_to_end.rs:23`, not a serving dependency
- [ ] Audit remaining load/content boundaries in `map.rs`; the split already recorded in Layer 7
      does not establish that the current file has no remaining misplaced values
- [ ] Audit map-type readers against file-backed values and protocol `MapKinds` (Layer 3);
      ownership by a compiled `asura-rules` constant is not the current implementation
- [ ] Remaining compiled content is listed in Layer 15; audit it without repeating that list here

## How to update this file

One box per commit. Tick the box, and append the commit hash and the evidence line - the command,
the count, or the capture - on the same line. A box is never ticked because the work is planned,
and completed task boxes do not change a layer's `audit pending` status.


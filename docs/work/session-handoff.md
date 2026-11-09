# Session handoff

Latest task update: 2026-11-08. Gameplay notes below retain their recorded acceptance and pending-verification status.

| Need | Document |
|---|---|
| Working instructions | [AGENTS.md](../../AGENTS.md) |
| Feature acceptance | [Project status](project-status.md) |
| Recorded owner decisions | [Project decisions](../reference/decisions/project-decisions.md) |
| Incident ledger and reasons behind working rules | [Development incidents](../history/development-incidents.md) |
| Earlier delivery/status records | [Infrastructure history](../history/infrastructure-milestones.md) |
| Server operations | [Running the server](../operations/running-the-server.md) |
| Planned work | [Project roadmap](project-roadmap.md) |
| The server being built, phase by phase | [Re-engineering plan](re-engineering-plan.md) |

## Completed documentation task

The [documentation consistency task](documentation-consistency.md) is complete for its recorded
scope. It preserves the recovered transcript context, completed passes, validation and remaining
intent/evidence questions. All 18 layers remain open. AGENTS.md, runtime behavior, configuration
and recordings were not changed by this pass; one source-comment reference follows the merged model.

## Latest gameplay session notes

- **Manual movement follow-up (2026-11-06).** Acceptance and the owner's exact feedback are maintained
  in [Project status](project-status.md#maps-and-movement). Remaining movement and synchronization
  behavior is unresolved; the feedback does not establish the size or cause of the remaining errors.
  The directional walker in crates/asura-world/src/pathfind/directional.rs is unfinished and must
  not be described as complete, satisfactory to the owner, or client-confirmed fixed.
  The owner also reported loss of confidence following repeated failures to launch a shell and
  the assistant asking them to restore access they had never withdrawn. The current instruction
  is to record this status in documentation and code comments only. Further functional work is paused.

- **Outpost login placement repair (2026-11-06), awaiting client confirmation.** Login at a saved
  doorway coordinate incorrectly selected the doorway arrival before the outpost spawn list.
  `Standing::arrival` no longer infers a doorway from coordinate equality: migration 0027 adds
  `t_transfer.arrival_from_map_id`, set only for a doorway transfer, and the receiving connection uses
  that source map to select its arrival. Login, world-map travel and `/tp` carry no doorway origin
  and choose a random outpost spawn. Ordinary outpost entry without a spawn list is refused with a
  reason instead of resuming a saved position.
- **Northlands load crash (2026-11-06), file id repaired, awaiting client confirmation.**
  `logs/world-8001.log:417262-417268` records the 146 -> 147 crossing, redirect, and entry at
  `(-11574, -16170)`, then disconnection. The owner's
  `C:/Users/Apo/Documents/Guild Wars/ArenaNet.log` at 2026-11-06 01:41:29 records
  `Assertion: found`, `Map.cpp(1762)`, after map file `0x8005ea96` failed to load and a default map
  was created. Decimal file id 2147871382 is already in HEAD's map 147 `basic.json` and
  `tools/map-config.sql`. The owner's live capture
  `recordings/client/raw/northlands-live-20261106-014922.jsonl` contains 1,028 packets and both
  crossings. Record 380 identifies destination 147; record 511 is StoC `0x0196`, whose first payload
  field is map file id **116025** (`0x0001C539`), followed by `(-11574, -16170)`, plane 0.
  The return load is destination 146 (record 801), file id 113021, `(-5434, 11799)`, plane 0
  (record 932). These are client-struct records with four-byte headers, not wire frames. Arrival
  coordinates agree with ASURA; Northlands' map file id did not. Map 147's `basic.json` now states
  116025, and world startup, on-demand loading, map listing and path dumping resolve file IDs from
  pack records only. A pack record with no file ID returns no usable
  file instead of substituting SQL. Auth and world now use `asura-config::world` for the pack's map
  descriptions; startup no longer seeds map-config.sql or installs SQL portal rows. The remaining
  128 legacy map descriptions and their spawn lists were migrated from a content-only snapshot to
  JSON folders under maps/unplaced, without assigning unmeasured campaigns or regions. Existing map
  files and the owner's deliberate explorable spawn deletions were preserved. New instance-table
  values remain null when unmeasured. No running map/portal path reads t_map_config, t_map_spawn or
  t_map_portal. Historical migrations and export tools still contain those tables; they are not the
  runtime content source. JSON had previously driven the instance table but not the blob ID sent to
  the client, which is why changing basic.json alone would not have fixed this crash.
  The bundled path graph also used the stale ID as its lookup key. Northlands was regenerated from
  the owner's local archive using file 116025 and its one entry replaced in `data/path-graphs.bin`;
  the other 141 entries were retained. This restores its graph binding with the corrected client ID.

- **Portal wall crossing repair (2026-11-06), awaiting client confirmation.** The accepted reports in
  `logs/world-8001.log:410655` and `:410664`, `(7277.4, 5417.9)` -> `(7184.3, 5307.3)`, intersect map
  148's authored wall at approximately `(7256.8, 5393.4)`, but no travel followed. The instance tick
  advanced the agent between reports without testing doorways; the movement handler then started the
  check from that advanced position instead of the preceding report. `Player::portal_position` now
  retains the preceding finite client position independently of ticks. Portal checks use the raw
  reported coordinates even when the simulation's movement budget clamps or refuses the report;
  a simulation correction cannot erase a client crossing. Finite stop corrections also advance it. Reports alone still delayed detection until the next client message, so
  `Instance::step` now checks every actual simulated movement segment and sends a crossing event to
  the owning connection, which starts travel ahead of ordinary broadcasts without another report or
  added timer. The owner clarified that touching the plane starts travel: finite wall intersection
  now includes endpoints and contact with the wall, while disjoint collinear segments do not trigger.
  The owner requested only boundary intersections: authored walls remain finite segments, and legacy
  single-coordinate doors now use the four edges of an axis-aligned box.
  `config/Server/portals.json` states its half extent; an invalid or absent value disables point boxes
  with a logged reason. Only the pack supplies doorways and arrivals; the SQL and compiled fallback tables were removed. Deleted explorable
  spawn lists are optional and are unrelated to detecting a doorway.

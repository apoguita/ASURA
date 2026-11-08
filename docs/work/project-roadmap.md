# Project roadmap

Future work and sequencing. Completed infrastructure milestones are recorded in [Infrastructure history](../history/infrastructure-milestones.md). The detailed plans retain their recorded open questions and acceptance requirements.

## Phase 2 - simulation

The detailed implementation handoff is `docs/work/world-simulation-plan.md`. Its first assignment
is configurable map spawns and portal arrivals with a small content manager, followed by isolated
instances, NPCs, combat, HeroAI, a Lua quest, and capacity measurement. Read its owner intent,
scope boundaries, acceptance checks, and unresolved decision gates before implementing a phase.
The manager technology and JSON authoring workflow have not been selected by the owner.

Existing login, travel and movement code is the starting point. Read [Project status](project-status.md)
for dated client results, unaccepted movement work and repairs awaiting verification; this plan
does not declare the current path accepted. The S1-S4 rows below describe architectural outcomes. Use the detailed handoff for the
current implementation order; its seven phases are distinct from the infrastructure milestones
and the packet-parity stages. None is marked implemented merely by recording this plan.

| milestone | outcome |
|---|---|
| S1 | Implement the selected scripting/behavior direction in [Scripting and behavior](../guidance/scripting-and-behavior.md); its unresolved Rust/Lua scope remains explicit |
| S2 | the world server builds and runs from this repository, with its own maps and player load/save |
| S3 | Read/write durable game state through asura-db; content readers are described once in [World pack files](../../config/World/README.md) |
| S4 | Instance teardown and bounded memory; the earlier return-to-pool wording remains subject to the lifecycle/reuse questions in [World model](../reference/game-world-model.md#unresolved-statements-requiring-review) |

## After M6

- Districts as capacity (a town that is full gains a second instance) - same machinery as
  explorable pools, different policy.
- Chat across worlds, if it is ever wanted.
- Observability worth keeping: per-world instance/player/RSS/tick metrics, exported and logged.

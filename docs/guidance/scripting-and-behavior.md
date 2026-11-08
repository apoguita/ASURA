# Scripting and behavior

Design discussion recorded 2026-11-01. This is a planning document, not an implemented subsystem.
The source for the decisions below is the owner's instructions in that discussion, not a claim
about the live game's internal design.

## Chosen: Rust simulation and Lua content scripts

The owner selected **Lua** for content scripting on 2026-11-01, replacing Rhai after reading both:
"Seems too new, not much documentation and examples and real world usage. I think ill do LUA
instead." Contributors should be able to edit text files and run them through ASURA without
installing a separate scripting interpreter or rebuilding the server for each content change. Lua is
intended to be embedded in the server; it has not been added to Cargo or the runtime yet.

What the change carries, recorded so it is not discovered later:

- A vendored Lua adds a C build requirement. Earlier scratch timing reports and their limits are
  retained in [HeroAI research](../research/heroai-server-port.md#earlier-scripting-experiment-report).
- The sandbox remains to be defined: no `io`, `os`, `debug` or `package`, no bytecode loading, an
  instruction hook and a memory ceiling. Lua supplies the parts; the policy is ours;
- quests gain suspension. Coroutines let a quest wait mid-flow, so the handler shape proposed below
  is one option rather than the only one.

The owner already has behavior-tree AI in Py4GW Reforged. Its game decisions are to be authored
in external Lua scripts; Rust supplies the behavior execution machinery and world operations.
BehaviorTree.CPP is the selected execution reference for the Rust node runtime; the HeroAI port
must handle the differences recorded in `docs/reference/decisions/0014-behavior-tree-execution.md`. That port has
not started. No external project becomes a runtime dependency; the dependency rule still applies.

## Simulation-wide boundary: mechanics and behavior

The owner's distinction applies to the entire game simulation, not only skills. External
scripts define game behavior: what happens, when, to whom, under which conditions, and what
happens next. The server implements mechanics: the supported operations that execute those
rules and change authoritative world state. Scripting is the primary place to author game
behavior, not an optional layer for choosing among complete hardcoded implementations.

Scripts can contain conditions, calculations, sequences, and event reactions. The boundary is
not simply data versus code: a script describes and requests behavior using supported server
operations; the server executes those operations. The server also enforces the constraints of
its mechanics. A script cannot create an operation the server does not provide.

| Domain | External scripts define behavior | Server mechanics execute |
|---|---|---|
| NPCs and AI | Who to attack, heal, follow, avoid, or help; priorities and conditions | World queries, behavior execution, and supported actor actions |
| Movement | Where and when to move, patrol routes, chase and retreat decisions | Pathfinding, movement, collision, and position updates |
| Combat and skills | Target rules, costs and timings, effects, reactions, and skill functionality | Resource accounting, cast scheduling, projectiles, damage/effect operations, and death state |
| Quests and missions | Objectives, prerequisites, progression, branches, success, and failure | Quest-state storage, event delivery, and supported state/reward operations |
| Spawns and encounters | Which actors appear, where, when, and how encounters progress | Actor creation/removal, placement, and lifecycle operations |
| Loot and rewards | What is awarded or dropped, to whom, under which rules | Supported random draws, item creation, inventory/currency changes, and persistence |
| Dialogue and interactions | What an NPC says, offered choices, conditions, and consequences | Interaction delivery, supported client messages, and world operations |
| Maps and instances | Encounter setup, entry reactions, scripted transitions, and completion rules | Map loading, instance lifecycle, placement, travel, and geometry operations |

These rows describe responsibilities, not existing APIs or claims that these mechanics have
already been implemented. The same boundary applies to other game behavior beyond this list.
For example, a script can declare an ambush after a quest checkpoint and a reward after the
ambushers die; the server must provide the checkpoint events, spawning, death events, and
reward operations. If the server offers no flight mechanic, scripts cannot make actors fly.

Rust retains the execution machinery, authoritative world state, persistence operations, and
packet construction. The selected Rust behavior-tree runtime executes behavior; it does not
require game-specific decisions to be hardcoded in Rust. Existing HeroAI decisions are behavior
to expose through the external scripting boundary, while the runtime remains a server mechanic.
The exact connection between scripts and that runtime is still open.

This simulation-wide distinction was clarified by the owner on 2026-11-02. The API, its names,
script formats, and event contract remain unselected. This records the intended boundary and
authorizes no runtime implementation. Skill examples and the Rurik review are in
[Skill mechanics and behavior research](../research/skill-mechanics-and-behavior.md).

The packet-stream objective remains binding for reproduction of the base game. The owner's wish
to support more complex custom content is a future capability, not permission to change existing
game behavior or invent protocol messages.

## Unresolved Rust/Lua boundary

| Recorded statement | Evidence | Unresolved scope |
|---|---|---|
| External Lua owns game behavior throughout simulation | Owner clarification, 2026-11-02; simulation-wide table above | Existing Rust placement rules are game-specific decisions, not only mechanical operations |
| `asura-rules` owns pure decisions needed by protocol answers | D25/D34 in [Layer decisions](../reference/decisions/layer-decisions.md); auth calls `placement` today | No recorded reconciliation establishes whether these decisions remain Rust-owned or become profile-authored |
| Lua constructs trees; Rust owns ticking and running state; Lua does not execute per-tick logic | The recorded technical/configuration split in HeroAI and custom skill behavior above | ADR 0014's migration checkpoint also calls for game-specific leaf decisions in Lua. Invocation timing and the meaning of "per-tick logic" are not defined |

This table preserves the conflict without selecting a binding API, callback schedule or migration.
Implementation work that depends on those choices needs the owner's answer; documentation cleanup does not.

## Skill declaration examples

| External skill behavior declares | Server mechanic executes |
|---|---|
| Which targets qualify and what conditions change the result | Target queries and validation against authoritative state |
| Resource requirements, activation, recharge, and duration values | Resource accounting, cast scheduling, interruption, and timers |
| Launch a projectile; on impact, damage the target | Projectile movement, hit resolution, damage calculation, and health changes |
| Apply regeneration while an enchantment lasts | Effect ownership, regeneration accounting, expiration, and removal |
| Limit eligible incoming damage while an effect lasts | Supported damage adjustment operations at the appropriate resolution stage |
| React to an attack by damaging the attacker | Event delivery and execution of the requested damage operation |
| Which skill an NPC chooses and who it targets | Execution of the chosen action through the same world operations |

These are skill-specific examples of the general boundary, not selected API names or a file
format. Rust behavior-tree execution remains a server mechanic; game-specific decisions belong
in external behavior scripts. How scripts connect to that runtime, and the exact events and
operations exposed to Lua, remain open. Scripts run through the server; they do not independently change client state or send
invented packets. See the simulation-wide boundary above.

## HeroAI and custom skill behavior

HeroAI's skill knowledge and game decisions are behavioral configuration supplied in external
Lua: skill selection, use conditions, target rules, priorities, combinations, resource-
management decisions, coordination, and build-specific tactics. A profile's author edits this
configuration, including for custom skills, without modifying or rebuilding Rust when the
required mechanics already exist. A skill's behavior configuration and HeroAI's rules for using
it are separate entries, both supplied by the selected profile.

The division is fixed (owner, 2026-11-02): **Rust owns the technical layer** - the behavior-tree
runtime and its tick, node kinds and execution semantics, per-agent tree state, halting and
reset, actor-relative queries, wrapped actions and calls, budgets, and error policy. **Lua is the
configuration layer**: it declares the behavioral configuration and constructs the tree from
Rust-provided node kinds and wrapped leaves. Lua does not tick, does not hold running node state,
and does not execute per-tick logic. Tree construction is configuration; execution is a Rust
mechanic. The Lua binding shape and the configuration format remain open.

A later acceptance case must add a custom skill using existing mechanics, define its behavior
and HeroAI usage in a copied profile, and demonstrate the NPC using it without editing or
rebuilding Rust. The original profile must remain unchanged. A new unsupported mechanic still
requires engine work; an unfamiliar skill alone is not a reason to hardcode its behavior.

## Behavior-tree source review

The pinned source comparison is in [HeroAI server port research](../research/heroai-server-port.md#behavior-tree-class-review-2026-11-01).


## Proposed: event-driven scripting

The discussion favors short handlers that react to events and request existing Rust actions.
The owner's final wording was "possible event driven scriptting"; the event model remains a
proposal rather than a settled implementation contract.

Candidate events discussed:

- A quest is accepted, advances, completes, or fails.
- An NPC interaction or dialogue choice occurs.
- An enemy dies or a named group is defeated.
- A character enters an area or an escort reaches a checkpoint.
- A relevant item is obtained or inventory changes.
- A timer expires.

These are content concepts, not registered event names or measured live-client triggers.
Delivery order, handler scheduling, and failure behavior are open.

Example sequence, independent of API syntax:

1. Accept the escort quest and start an existing escort behavior.
2. Reach a checkpoint and spawn an ambush group.
3. Defeat the group and let the escort continue.
4. Reach the destination and complete the objective.

Rust handles the walking and fighting throughout. A companion that follows the party instead of
leading it uses a different available behavior; it does not require scripts to calculate movement.

## Quest content

The owner described ordinary quests as combinations of these requirements:

| Requirement | Content describes | Rust support needed |
|---|---|---|
| Fetch or loot items | Item identity and required count | Inventory queries and relevant events |
| Kill a boss or pack | Enemy or group identity and completion condition | Death events and group tracking |
| Reach a destination | Area or checkpoint and conditions | Area-entry or checkpoint detection |
| Escort an NPC | Route, checkpoints, success and failure conditions | Movement, behavior trees, and combat |
| Have an NPC accompany the party | Companion identity and when it follows or leaves | Following and combat behavior |
| Require earlier progress | Prerequisites and follow-up quests | Quest-state queries and updates |

Standard objectives can be combined into sequences, requirements that all must be met, or
alternative ways to succeed. Lua provides room for special branches and sequences beyond those
ordinary combinations. A new sequence of existing actions is distinct from a new engine mechanic.

Main quests, secondary quests, and quest chains share these concepts. An NPC's existence, movement,
and available quests are separate: a roaming NPC can offer a quest, and a permanent villager can
offer different quests according to progress.

Whether ordinary objectives are authored as data, Lua, or both is not decided. JSON was discussed
as a possible data format, not selected as the storage format. A map-canvas editor and state
graphs were discussed as authoring approaches, not approved implementation work. The initial
map/portal editing screen is part of the handoff in `docs/work/world-simulation-plan.md`.

## Map spawns and layouts

The content design discussed separates the following concepts. These are planning terms, not
existing schema tables or Rust types.

| Concept | Meaning |
|---|---|
| Agent definition | Model, faction, stats, available services, and assigned behavior |
| Placement | Position and facing, or a permitted area in which to spawn |
| Group | Agents belonging to a pack, patrol, reinforcement wave, or ambush |
| Movement assignment | Standing, roaming within an area, or following a route |
| Spawn conditions | Always present, quest-dependent, or created after an event |
| Lifetime | Despawn conditions, whether death persists, and any respawn delay |
| Quest relationships | Quests an NPC offers or accepts, subject to availability conditions |

Examples covered: a map's normal enemy population, roaming service NPCs, side-quest givers,
quest companions, checkpoint ambushes, and reinforcements after a boss dies.

Layouts can be authored as exact placements, spawn areas, or named population sets such as a
peaceful village and an occupied village. These are available approaches, not a decision to use
random placement or replace measured base-game layouts.

Phasing means changing the population or visibility according to progress. Layering means separate
populations of a location. Neither follows automatically from selecting Lua. Character, party,
and instance state have different scopes; the population-selection policy is not decided.

## Open questions

- Do ordinary quests use objective definitions, Lua handlers, or both?
- Does possessing an item satisfy an objective, or must it be obtained during the quest?
- Does an explorable population follow the leader, combined party quests, or a per-quest rule?
- Which progress survives logout, and which belongs only to the running encounter or instance?
- How are separate instances, phases, and layers identified and selected?
- What game actions and state queries does the scripting API expose?
- How are events ordered, handlers scheduled, and duplicate triggers handled?
- What happens after a script error or execution-limit failure, including actions already applied?
- What execution, data-size, and access limits apply to scripts and Rust callbacks?
- When does changed content take effect, and what happens to active quests during reload?
- Which runtime version, Cargo features, content format, and packaging are used?

Earlier scripting timings are an unverified experiment report, retained once in
[HeroAI research](../research/heroai-server-port.md#earlier-scripting-experiment-report). They do not establish instance capacity.

## Current code and reading

- `crates/asura-world/src/instance.rs`: the current instance, player slots, movement tick, and
  registry keyed by map id. It does not establish the future multi-instance or layering policy.
- `crates/asura-world/Cargo.toml`: no scripting runtime dependency at the time of this discussion.
- `docs/reference/game-world-model.md`: game nouns and the owner's existing world-model decisions.
- `docs/guidance/architecture.md`: service ownership and the content database's architectural role.
- `docs/reference/decisions/0014-behavior-tree-execution.md`: selected execution reference and HeroAI node mappings.
- `docs/research/heroai-server-port.md`: headless combat policy, client assumptions, and the
  100-instance / 200-agent scaling review.
- `docs/work/world-simulation-plan.md`: detailed implementation handoff, owner intent, the early
  map/portal manager, seven phases, acceptance checks, and questions to ask before dependent work.
- `docs/work/control-panel-plan.md`: the expanded panel for server operations, capture sessions,
  guided tests, recordings, and configuration/content management (owner, 2026-11-02).
- [mlua, the Lua binding for Rust](https://docs.rs/mlua/latest/mlua/).
- [The Lua 5.4 reference manual](https://www.lua.org/manual/5.4/): a debug hook counts instructions,
  and an instruction count does not bound the time spent inside a Rust function.

This records the selected language and the design under discussion. It does not authorize a
runtime integration, AI port, schema migration, content conversion, or change to live behavior.

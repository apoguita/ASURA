# Documentation index

Start with [AGENTS.md](../AGENTS.md), then choose the assigned subject below. For a continuing
session, read the [latest session notes](work/session-handoff.md); check [project status](work/project-status.md)
when the task depends on what was implemented or client-confirmed.

## Choose a task

| Task | Read |
|---|---|
| Run or diagnose the server | [Running the server](operations/running-the-server.md) |
| Understand component boundaries | [Architecture and component responsibilities](guidance/architecture.md) |
| Work on a packet or load sequence | [Recorded behavior reproduction](reference/client-protocol.md#recorded-behavior-reproduction), then [Client protocol](reference/client-protocol.md), [instance initialization](reference/instance-initialization.md) |
| Find packet coverage or unnamed messages | [Packet coverage](work/packet-coverage.md), [unnamed packets](work/unnamed-packets.md) |
| Work on packs and configuration | [Game packs](guidance/game-packs.md), then the README beside the files being edited |
| Work on scripting or AI | [Scripting and behavior](guidance/scripting-and-behavior.md), then the assigned plan/research record |
| Review documentation cleanup results | [Documentation consistency task](work/documentation-consistency.md) |
| Continue planned work | [Project roadmap](work/project-roadmap.md), then the assigned detailed plan |
| Investigate an earlier measurement or failure | The research/history record linked from the relevant subject |

The folders classify a document's purpose. Their placement does not settle contradictory statements
or change a decision's recorded status. Older status claims remain qualified by their original dates.

## Active guidance

Project responsibilities and authoring descriptions.

- [Architecture](guidance/architecture.md)
- [World content and scripting](guidance/scripting-and-behavior.md)
- [Selectable self-contained game profiles](guidance/game-packs.md)

## Operation

Procedures to run, with platform and research-tool prerequisites.

- [Running ASURA](operations/running-the-server.md)
- [Running the server on Linux](operations/running-on-linux.md)
- [The initialization capture: what to run, and what it answers](operations/capturing-instance-initialization.md)
- [The client patcher](../client-patcher/README.md)
- [Recordings - public research evidence](../recordings/README.md)

## Reference

Lookup facts, models and catalogues; source qualifications remain in each document.

- [Glossary](reference/glossary.md)
- [Protocol](reference/client-protocol.md)
- [The initialization protocol: everything a client is fed when it enters an instance](reference/instance-initialization.md)
- [The inventory model, and what measuring it changed](reference/inventory-model.md)
- [The world model: account, character, instance, agent](reference/game-world-model.md)

- [The map list](reference/map-catalogue.md)
- [A reference session, measured](reference/captures/login-session-2026-10-26.md)
- [Server settings](../config/Server/README.md)
- [World packages](../config/World/README.md)
- [The Vanilla pack](../config/World/Vanilla/README.md)

## Research

Investigations, source comparisons, measurements and unresolved findings.

- [What the GW2 microservices talk changes in this design](research/gw2-service-design-lessons.md)
- [Multi-machine capability, proven on one machine (S6)](research/multi-host-routing-experiment.md)
- [Rurik: the feature gap, and the port queue](research/rurik-capabilities-and-gaps.md)
- [The surrounding packets: what a live stream carries that this tree ignores](research/client-session-packet-analysis.md)
- [HeroAI headless: server reuse and scaling](research/heroai-server-port.md)
- [Skill mechanics and behavior research](research/skill-mechanics-and-behavior.md)

## Work tracking

Recorded status, session context and intended work. Plans are not claims of implementation.

- [Features](work/project-status.md)
- [Session handoff](work/session-handoff.md)
- [Documentation consistency task](work/documentation-consistency.md)
- [Project roadmap](work/project-roadmap.md)
- [World simulation implementation handoff](work/world-simulation-plan.md)
- [ASURA control panel: operations, research, tests, and content](work/control-panel-plan.md)
- [Rework layers: the checklist](work/layer-rework-checklist.md)
- [Stream parity: every message a live client exchanged, and what we do with it](work/packet-coverage.md)
- [The game headers still to be named, and the test that names each](work/unnamed-packets.md)

## History

Dated reports, completed plans, incidents and earlier attempts.

- [Development incidents and working-rule history](history/development-incidents.md)
- [Infrastructure milestones and earlier session records](history/infrastructure-milestones.md)
- [What we learned from GWPS (research record)](history/gwps-project-origins.md)
- [Starting ASURA's services: research for a decision](history/service-supervisor-research.md)
- [Implementation plan: the seven improvements from the analysis](history/service-supervisor-delivery-plan.md)
- [The packet-parity attempt: objective, what was done, and why it failed](history/packet-parity-attempt-2026-10-31.md)
- [Status: what works, what is a stub](history/gwps-parity/handler-status.md)
- [GWPS parity audit](history/gwps-parity/behavior-audit.md)
- [The parity work queue](history/gwps-parity/completed-work-queue.md)
- [The state of parity, measured](history/gwps-parity/parity-snapshot-2026-10-31.md)
- [The database schema, diffed against the reference's](history/gwps-parity/database-schema-comparison.md)
- [The GM command surface, diffed against the reference's](history/gwps-parity/chat-command-comparison.md)
- [The reference's test plan, mapped onto the checks in this repository](history/gwps-parity/reference-test-coverage.md)
- [Documentation organization proposal](history/reviews/documentation-organization-2026-11-07.md)

## Decision records

Recorded choices and their reasoning, including accepted, superseded and unresolved entries.

- [ADR 0001 - Write the control plane in Rust, keep the world servers in C](reference/decisions/0001-control-plane-language.md)
- [ADR 0002 - A separate project instead of another fork of GWPS](reference/decisions/0002-project-separation.md)
- [ADR 0003 - Simulation is in ASURA's scope, deferred behind the control plane](reference/decisions/0003-simulation-scope.md)
- [ADR 0004 - Port AzerothCore's server structure](reference/decisions/0004-server-structure.md)
- [ADR 0005 - GWPS becomes reference only; ASURA is where things are built and tested](reference/decisions/0005-gwps-reference-boundary.md)
- [ADR 0006 - AzerothCore is the design authority](reference/decisions/0006-structural-design-authority.md)
- [ADR 0007 - Rust, translated from AzerothCore's C++](reference/decisions/0007-whole-stack-rust.md)
- [ADR 0008 - Database vendor: PostgreSQL, a justified deviation](reference/decisions/0008-postgresql.md)
- [ADR 0009 - Crypto primitives: a hand-written ARC4, and a big-integer crate for the exponentiation](reference/decisions/0009-protocol-cryptography.md)
- [ADR 0010 - GWPS is the floor, not a reference](reference/decisions/0010-gwps-behavior-coverage.md)
- [ADR 0011 - Rurik joins GWPS as a recorded source, not a dependency](reference/decisions/0011-rurik-source-policy.md)
- [ADR 0012 - Eden is a recorded source: a client, read as evidence for what the client does](reference/decisions/0012-eden-source-policy.md)
- [ADR 0013 - Py4GW Stealth is this project's link to the client](reference/decisions/0013-py4gw-client-research-policy.md)
- [ADR 0014 - BehaviorTree.CPP is the behavior-tree execution reference](reference/decisions/0014-behavior-tree-execution.md)
- [Layer decisions: why the layers are arranged as they are](reference/decisions/layer-decisions.md)
- [Recorded project decisions](reference/decisions/project-decisions.md)

## Consolidation record

The documented cleanup is complete; scope, checks and remaining intent/evidence questions are in
[Documentation consistency task](work/documentation-consistency.md). The original twelve merge
groups and their disposition are in the [organization review](history/reviews/documentation-organization-2026-11-07.md#disposition-of-the-twelve-groups-2026-11-08).

World-model definitions and rule evidence are consolidated in reference/game-world-model.md.
Unresolved gameplay or Rust/Lua statements remain labeled in their subject documents; this is
not a claim of runtime compliance, full factual verification or client acceptance.
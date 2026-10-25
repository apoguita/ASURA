# ADR 0003 — Simulation is in ASURA's scope, deferred behind the control plane

- **Status:** Accepted (owner's direction, 2026-10-25)
- **Date:** 2026-10-25
- **Context:** ASURA is meant to become the server stack for a private Guild Wars server, not only
  the authentication and allocation half of it. Simulation — agents, movement, pathfinding, combat,
  drops — is therefore in its future. It is also the largest body of work in the stack, and it
  already exists in C inside GWPS, working well enough to be the reference for the client protocol.

Meanwhile the lessons from GWPS are all about the control plane: identity, allocation, process
model, the cost of an instance, and the hang that came from building a server inside a travel.
Those have to be right before simulation is anything but premature optimisation.

## Decision

ASURA owns simulation as part of its long-term scope, and implements it in a later phase:

- **Phase 1 (now):** the base structure. Auth service, schema, registry and pools, placement, and
  a world server that owns its own connections. Simulation stays in the existing C world server,
  reached through the database rather than a private protocol.
- **Phase 2 (later):** simulation becomes part of this project. Whether it is a Rust port or the C
  implementation adopted as an owned component is decided at S1 of the roadmap, on its merits, with
  no assumption inherited from phase 1.

Two constraints make the deferral safe rather than merely postponed:

1. **The boundary is a schema, not a library.** The database contract is explicit and versioned,
   so the simulation can be replaced or moved without the rest of the stack changing.
   No shared headers, no in-process calls, no "just link it in for now".
2. **No phase-1 decision may foreclose phase 2.** In particular, the control plane must not assume
   that maps, players or state live in its own address space, and must not grow logic that only
   works because the far side is a C program.

## Why this order

- The control plane is small and its failures are cheap to test; the simulation is large and its
  failures are expensive to diagnose.
- Every scaling measurement that exists (12 MB per instance, the duplicated `Gw.dat` index, the
  per-instance database connection, the thread-per-map model) is about the simulation's placement,
  not its content. Getting the placement right first means the simulation is written once, into a
  shape that already supports hundreds of instances.
- The one client-visible defect in the project's history was an allocation defect. Fixing
  allocation before writing more simulation is fixing the thing that actually broke.

## Consequences

- `docs/01-scope.md` describes a *phase 1* boundary; read it with this ADR in hand.
- `docs/02-architecture.md` keeps the world server as an external process, described by protocol.
- `docs/roadmap.md` carries phase 2 milestones (S1–S4) that must not be silently dropped.
- ADR 0001 (Rust) answers the control plane's language only. The simulation's language is explicitly
  open, and S1 is the milestone that closes it.
- GWPS's `GameSrv.c` is not waste: it is the seed of phase 2 and the behavioural reference for it.

## Alternatives rejected

- **Simulation first, control plane later.** Rejected: the scaling and allocation problems are the
  ones that already bit, and simulation built on the current shape would inherit them.
- **Permanently split: ASURA is control plane only, worlds live in GWPS forever.** Rejected by the
  owner: one project, one stack is the intent.
- **Bring the simulation in immediately, as-is.** Rejected: it would import the single-process
  assumptions (thread per map, index per instance, construction on demand) into the new project at
  exactly the moment they are cheapest to leave behind.

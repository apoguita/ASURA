# Documentation consistency task

Status: documented cleanup complete, 2026-11-08. This records the scope, work and remaining
intent/evidence questions. It is not another architecture, rule ledger or feature-status page.

## Task and scope

The owner's instruction is "removing duplicate and conflicting facts, we will define behaviors
later, i need the docs consistent" (`lost session.txt:414`). Each fact gets one maintained home;
other documents link there. The owner rejected a second component inventory (`:381`) and deferred
AGENTS.md changes (`:325`, `:358`). Code establishes implementation, not conceptual intent.

The scope includes removing duplicates, correcting stale implementation descriptions, separating
current status from dated evidence, and making unresolved intent explicit. It does not certify
live-game rules or authorize runtime/configuration changes. No layer is closed (owner, 2026-11-08).
The earlier invented restriction that gameplay questions require a separate compulsory review
was withdrawn in the transcript (`:95-99`); this cleanup introduces no such restriction.

## Recovery and surviving work

The task context was lost; edits survived in the working tree. Recovery on 2026-11-08 read the
root `lost session.txt`, created this task record and linked it from the index and handoff.
The transcript ends with the first pack-fact pass and remaining component/status checks (`:416-425`).
The resumed architecture/status passes are recorded below.

Existing source/configuration changes and document moves from other work were preserved. They
were not reset, staged, committed or tested as this cleanup's work. The raw transcript and all
recordings remain unchanged. One source-comment link was updated when the rule ledger was retired;
that edit changes no Rust statement or runtime behavior.

## Completed passes

| Order | Subject | Result and evidence |
|---|---|---|
| Organization | Names, purpose folders, index and handoff split | Original mapping and all twelve merge-group dispositions are in history/reviews/documentation-organization-2026-11-07.md |
| 1 - checked | Architecture and ownership | One component inventory; gateway/auth routing corrected. Reviewed registry/reconciliation, instance creation/persistence, write paths, deployment, startup/shutdown, retries and failures against serving code. Unresolved code/intent differences remain in architecture's final section |
| 2 - checked | Status, handoff, roadmap, checklist and plans | All 18 layers remain audit pending. Corrected movement acceptance, pending repairs, deletion, starter maps, portal fallbacks, instance-table moves, dependency roles and source/test references. Ignored refused-walk test is not treated as proof. Missing T_PREFIX_COMMIT evidence remains an open checklist item |
| 3 - checked | World model and rule ledger | Merged into reference/game-world-model.md; retired redundant ledger and updated references, including one source comment. Preserved 220 E-rows/IDs and dated assertions; corrected malformed R11/R12 row and superseded R27 wording. Definitions, source-only findings and unresolved statements are distinct; no document declares itself superior to AGENTS |
| 4 - checked | Scripting, packs, HeroAI, skills and ADR 0014 | General responsibility description owned by scripting guide; ADR retains execution mappings and research retains source findings. Rust/Lua placement/callback ambiguity is explicit. Earlier Lua/Rhai scratch timings have no retained reproducer and are labeled unreproduced in research. Plans do not establish runtime integration |
| 5 - checked | Framing, initialization, capture analysis, coverage and naming | Fixed-only and counted framing reconciled against frame.rs. Initialization owns the current stage sequence; peer/tick gating and pack-dependent batching replace stale universal claims. Load-delay cause remains unproved. Compared coverage names/statuses with all 682 current game catalogue rows; all 17 naming-queue headers remain unnamed. Struct sizes are not certified wire lengths |
| 6 - checked | Operations, remaining references/research and directory READMEs | Corrected gateway route, seed/schema distinction, capture token prerequisites, configuration inventory and obsolete glossary terms. Inventory reference keeps dated divergences without authorizing them; removed an unsupported byte-equality claim after checking the current weaker test. Historical reports retain dates/status; Rurik queue is a proposal, not current assignments. No new summary file duplicates their evidence |
| 7 - checked | Index, incoming references and twelve merge groups | Index routes to maintained subjects and results. Organization review records each group's disposition, including historical reports deliberately retained separately. Remaining disagreements are in the subject references, not silently resolved by moving or merging files |

## One home per subject

| Subject | Maintained home |
|---|---|
| Components, connection ownership and current serving limits | guidance/architecture.md |
| Implemented pack layout/readers | config/World/README.md |
| Machine/client-value file inventory | config/Server/README.md |
| Intended profiles and saved-state isolation | guidance/game-packs.md |
| General mechanics/behavior responsibility | guidance/scripting-and-behavior.md |
| World definitions, assertions and rule evidence | reference/game-world-model.md |
| Wire layouts and framing | reference/client-protocol.md |
| Initialization sequence | reference/instance-initialization.md |
| Feature acceptance | work/project-status.md |
| Session-specific context | work/session-handoff.md |
| Future sequencing and detailed work | work/project-roadmap.md -> assigned plan/checklist |
| This task's completion and limits | work/documentation-consistency.md |

Paths omit docs/ except config/ paths. This table routes readers; it does not repeat the subject facts.

## Remaining intent and evidence questions

These do not reopen completed consolidation work or authorize implementation:

- Architecture's "Scope and boundary statements awaiting review" records current code/intent gaps.
- World model's "Unresolved statements requiring review" records travel gating, load/arrival scope,
  party truncation, activation timing, instance reuse/scope, doorway fallback and quest placement.
- Scripting guide's "Unresolved Rust/Lua boundary" records placement ownership and callback timing.
- Layer/checklist audits, missing historical test evidence and client verification remain open in
  their maintained trackers; no layer was accepted by this task.
- Source-only and wiki findings remain qualified by their witnesses. This cleanup did not reread
  every external source, remeasure the game, certify every inherited byte/number or claim parity.
- AGENTS.md remains unchanged; any later rewrite needs an owner assignment.

## Validation

Final check, 2026-11-08: all 66 repository Markdown files were checked for local file links and
section anchors; 241 references resolve. All 65 Markdown files outside recordings/ pass ASCII,
trailing-whitespace and code-fence checks. The unchanged archive README retains its original
non-ASCII text; the repository ASCII script excludes recordings/ and flags only unchanged
lost session.txt. All 220 E-IDs are preserved uniquely and all 18 layer rows remain audit pending.

Coverage names/statuses agree with the current game packet catalogue; all 17 naming-queue
headers remain unnamed. Source reads establish the documented implementation corrections,
not new passing test runs or client acceptance. The retired inventory byte-equality test name
is explicitly identified as absent rather than represented as current proof.

The prescribed test-touched.ps1 scope inspection succeeds with a process-local safe-directory
setting. Its commands cover pre-existing crate changes, so they were not run for this text-only
pass. No server, client, capture, migration, crate test, CI job or deployment was started.

## Resume point

No cleanup row remains assigned. Read the relevant unresolved-subject table or feature tracker
when the owner assigns further work; do not restart the organization pass or infer a gameplay fix.
# Documentation organization proposal

Prepared 2026-11-07. This replaces the earlier review with the file-organization proposal requested by the owner. It proposes names, locations, purpose, merge candidates and a reading path. It does not resolve game or architecture decisions.

## Applied organization

Applied 2026-11-07: files were grouped and renamed, references updated, the handoff split, and the short index created. The scope/responsibility document has been consolidated into guidance/architecture.md. World-model definitions and rule evidence were consolidated on 2026-11-08 in reference/game-world-model.md; the redundant ledger was retired with its evidence IDs preserved. The original proposal below is retained as history; the completed disposition at the end records how each merge group was handled. Unresolved intent remains explicit in the maintained subject documents.

## Original proposal scope

63 existing Markdown files are mapped: the 61 files in the original inventory, this review, and recordings/README.md. This paragraph describes the original proposal snapshot before the applied organization recorded above. Proposed destinations describe organization, not the correctness or authority of every sentence presently in a file.

Ordinary topic documents lose their chronological number prefixes. ADR numbers remain as stable decision identifiers. README.md stays where a reader expects it beside a component or data directory. There is one proposed new navigation page, docs/README.md; it is an index, not another manual. These are proposed naming choices for this reorganization, not new standing project rules.

## Purpose and folders

| Purpose | Proposed home | What a reader finds |
|---|---|---|
| Active guidance | docs/guidance/ | Project responsibilities and authoring conventions used while making changes. AGENTS.md remains at the root. |
| Operation | docs/operations/ | Procedures a person runs: startup, platform setup and capture tasks. Tool/data READMEs stay beside their files. |
| Reference | docs/reference/ | Facts to look up: terminology, layouts, models, catalogues and decision records. |
| Research | docs/research/ | Investigations, comparisons, measurements, hypotheses and their limits. |
| History | docs/history/ | Completed plans, old snapshots, incidents, failed attempts and completed reviews. |
| Work tracking | docs/work/ | Current status, session handoff, roadmap, outstanding tasks and implementation plans. |

## Current path -> proposed folder and descriptive filename

The purpose column is the primary destination for the file. Mixed files have section splits below. Repeated destinations are merge candidates, not permission to concatenate contradictory text or overwrite one file with another.

| Current path | Proposed path | Purpose | Merge group | Explanation |
|---|---|---|---|---|
| AGENTS.md | AGENTS.md | Active guidance | - | Keep the discovery filename and location. Organization does not rewrite its rules. |
| README.md | README.md | Active guidance | M01 | Short project entry; links to the documentation index instead of repeating the manual. |
| MILESTONES.md | docs/work/project-status.md | Work tracking | M02 | Name it for its feature/acceptance list, rather than infrastructure milestones. |
| docs/HANDOFF.md | docs/work/session-handoff.md | Work tracking | M02,M03,M04 | Keep current session state here; proposed section destinations are listed below. |
| docs/00-glossary.md | docs/reference/glossary.md | Reference | M03 | Definitions and terminology. |
| docs/01-scope.md | docs/guidance/architecture.md | Active guidance | M03 | Merge candidate with architecture: responsibilities and boundaries. |
| docs/02-architecture.md | docs/guidance/architecture.md | Active guidance | M03 | Merge candidate with scope; no architectural choice follows from this proposed merge. |
| docs/03-protocol.md | docs/reference/client-protocol.md | Reference | M05 | Protocol facts and layouts; dated experiments are separate sections/candidates, not additional active specifications. |
| docs/04-from-gwps.md | docs/history/gwps-project-origins.md | History | - | Earlier project/migration reasoning. |
| docs/05-gw2-lessons.md | docs/research/gw2-service-design-lessons.md | Research | M03 | Source analysis; adopted conclusions can be linked from architecture. |
| docs/06-reference-session.md | docs/reference/captures/login-session-2026-10-26.md | Reference | - | A dated measured session, named with the subject and date. |
| docs/07-status.md | docs/history/gwps-parity/handler-status.md | History | M06 | Already labeled historical; distinguish it from current project status. |
| docs/08-next-inventory-model.md | docs/reference/inventory-model.md | Reference | M07 | Replace the obsolete next-work title; separate its measurements from any remaining backlog. |
| docs/09-parity-audit.md | docs/history/gwps-parity/behavior-audit.md | History | M06 | Historical behavior comparison. |
| docs/10-parity-queue.md | docs/history/gwps-parity/completed-work-queue.md | History | M06 | Historical queue; the filename signals that this is not the active assignment list. |
| docs/11-parity-state.md | docs/history/gwps-parity/parity-snapshot-2026-10-31.md | History | M06 | Dated parity reading. |
| docs/12-schema-parity.md | docs/history/gwps-parity/database-schema-comparison.md | History | M06 | Historical schema comparison. |
| docs/13-command-parity.md | docs/history/gwps-parity/chat-command-comparison.md | History | M06 | Historical command comparison. |
| docs/14-test-plan.md | docs/history/gwps-parity/reference-test-coverage.md | History | M06 | Historical coverage mapping, not the current testing policy. |
| docs/15-service-startup.md | docs/history/service-supervisor-research.md | History | M04 | Research for the startup decision already made. |
| docs/16-implementation-plan.md | docs/history/service-supervisor-delivery-plan.md | History | M04 | Delivered infrastructure plan; remove generic implementation-plan ambiguity through naming. |
| docs/17-running-on-linux.md | docs/operations/running-on-linux.md | Operation | M04 | Platform-specific setup details. |
| docs/18-multi-machine.md | docs/research/multi-host-routing-experiment.md | Research | M04 | Measured second-address experiment and remaining validation, rather than a complete deployment manual. |
| docs/19-running.md | docs/operations/running-the-server.md | Operation | M04 | Main operating runbook. |
| docs/20-rurik-gap.md | docs/research/rurik-capabilities-and-gaps.md | Research | M08 | Keep the source comparison; proposed work ordering is a section-split candidate. |
| docs/21-surrounding-packets.md | docs/research/client-session-packet-analysis.md | Research | M05 | Flow analysis and measured omissions; duplicated task status is a split candidate. |
| docs/22-world-model.md | docs/reference/game-world-model.md | Reference | M09 | Definitions and described game behavior; merge candidate for overlapping ledger material. |
| docs/23-initialization-protocol.md | docs/reference/instance-initialization.md | Reference | M05 | Keep load sequence/detail discoverable by subject. |
| docs/24-stream-parity.md | docs/work/packet-coverage.md | Work tracking | M05 | Live-message handling coverage and gaps; capture facts retain their citations. |
| docs/27-packet-parity-attempt.md | docs/history/packet-parity-attempt-2026-10-31.md | History | - | Failed branch/experiment, explicitly dated. |
| docs/28-headers-to-name.md | docs/work/unnamed-packets.md | Work tracking | M05 | Naming backlog and the actions needed for evidence. |
| docs/29-initialization-capture.md | docs/operations/capturing-instance-initialization.md | Operation | M05 | Procedure to run, rather than another initialization specification. |
| docs/30-world-content-and-scripting.md | docs/guidance/scripting-and-behavior.md | Active guidance | M10 | Chosen authoring/execution responsibilities; embedded source reviews are split candidates. |
| docs/31-heroai-server-feasibility.md | docs/research/heroai-server-port.md | Research | M10 | Pinned source findings and capacity limits. |
| docs/32-world-simulation-plan.md | docs/work/world-simulation-plan.md | Work tracking | M08,M10 | Intended implementation phases and acceptance work. |
| docs/33-control-panel-design.md | docs/work/control-panel-plan.md | Work tracking | M08 | Requested/proposed product work, not an existing operating manual. |
| docs/34-self-contained-profiles.md | docs/guidance/game-packs.md | Active guidance | M11 | Pack model and separation requirements; unfinished implementation choices stay identified. |
| docs/35-skill-mechanics-and-behavior.md | docs/research/skill-mechanics-and-behavior.md | Research | M10 | Keep the skill examples/source review; repeated boundary wording is a merge candidate. |
| docs/36-game-world-model.md | docs/reference/game-world-model.md | Reference | M09 | Candidate for combining its assertions/evidence with the world model; questions remain labeled, not answered by moving text. |
| docs/37-rework-layers.md | docs/work/layer-rework-checklist.md | Work tracking | M08 | Tasks and completion evidence. |
| docs/38-layer-decisions.md | docs/reference/decisions/layer-decisions.md | Reference | M03,M11 | Decision record; moving it does not mark accepted decisions historical or rescind them. |
| docs/39-map-list.md | docs/reference/map-catalogue.md | Reference | - | Lookup catalogue. |
| docs/roadmap.md | docs/work/project-roadmap.md | Work tracking | M08 | Overall sequencing; completed milestone accounts are section-split candidates. |
| docs/adr/0001-rust.md | docs/reference/decisions/0001-control-plane-language.md | Reference | M12 | Retain the ADR number for existing citations. This is a decision record, with its existing status retained. |
| docs/adr/0002-separate-project.md | docs/reference/decisions/0002-project-separation.md | Reference | M12 | Retain the ADR number for existing citations. This is a decision record, with its existing status retained. |
| docs/adr/0003-simulation-in-scope.md | docs/reference/decisions/0003-simulation-scope.md | Reference | M12 | Retain the ADR number for existing citations. This is a decision record, with its existing status retained. |
| docs/adr/0004-port-azerothcore-structure.md | docs/reference/decisions/0004-server-structure.md | Reference | M12 | Retain the ADR number for existing citations. This is a decision record, with its existing status retained. |
| docs/adr/0005-gwps-reference-only.md | docs/reference/decisions/0005-gwps-reference-boundary.md | Reference | M12 | Retain the ADR number for existing citations. This is a decision record, with its existing status retained. |
| docs/adr/0006-azerothcore-is-the-reference.md | docs/reference/decisions/0006-structural-design-authority.md | Reference | M12 | Retain the ADR number for existing citations. This is a decision record, with its existing status retained. |
| docs/adr/0007-rust-translation.md | docs/reference/decisions/0007-whole-stack-rust.md | Reference | M12 | Retain the ADR number for existing citations. This is a decision record, with its existing status retained. |
| docs/adr/0008-database-vendor.md | docs/reference/decisions/0008-postgresql.md | Reference | M12 | Retain the ADR number for existing citations. This is a decision record, with its existing status retained. |
| docs/adr/0009-crypto-primitives.md | docs/reference/decisions/0009-protocol-cryptography.md | Reference | M12 | Retain the ADR number for existing citations. This is a decision record, with its existing status retained. |
| docs/adr/0010-gwps-is-the-floor.md | docs/reference/decisions/0010-gwps-behavior-coverage.md | Reference | M12 | Retain the ADR number for existing citations. This is a decision record, with its existing status retained. |
| docs/adr/0011-rurik-is-a-source.md | docs/reference/decisions/0011-rurik-source-policy.md | Reference | M12 | Retain the ADR number for existing citations. This is a decision record, with its existing status retained. |
| docs/adr/0012-eden-is-a-source.md | docs/reference/decisions/0012-eden-source-policy.md | Reference | M12 | Retain the ADR number for existing citations. This is a decision record, with its existing status retained. |
| docs/adr/0013-py4gw-stealth-is-our-link-to-the-client.md | docs/reference/decisions/0013-py4gw-client-research-policy.md | Reference | M12 | Retain the ADR number for existing citations. This is a decision record, with its existing status retained. |
| docs/adr/0014-behavior-tree-execution.md | docs/reference/decisions/0014-behavior-tree-execution.md | Reference | M12 | Retain the ADR number for existing citations. This is a decision record, with its existing status retained. |
| client-patcher/README.md | client-patcher/README.md | Operation | M04 | Keep the instructions beside the patcher. Link from the operations runbook. |
| config/Server/README.md | config/Server/README.md | Reference | M11 | Keep settings documentation beside the files it explains. |
| config/World/README.md | config/World/README.md | Reference | M11 | Keep the package-directory guide beside installed packs. |
| config/World/Vanilla/README.md | config/World/Vanilla/README.md | Reference | M11 | Keep base-pack-specific documentation inside its pack. |
| recordings/README.md | recordings/README.md | Operation | M05 | Keep archive/capture handling instructions beside the evidence; recordings stay in place. |
| docs/reviews/2026-11-07-documentation-review.md | docs/history/reviews/documentation-organization-2026-11-07.md | History | - | This proposal after the reorganization; it does not become an operating guide. |

## Files covering the same subject: merge candidates

These groups identify what overlaps and the unit of consolidation. A shared subject is not a reason to put procedures, history, evidence and guidance into one large file.

| Group | Subject | Existing files / sections | Proposed consolidation |
|---|---|---|---|
| M01 | Project entry | README.md; proposed docs/README.md | README gives a short introduction; docs/README is one navigation page. Replace repeated manuals with links. |
| M02 | Current status and handoff | MILESTONES.md; current-state sections of HANDOFF.md; status paragraphs elsewhere | Feature acceptance belongs in project-status.md; session-specific context belongs in session-handoff.md. Remove repeated copies when consolidation is applied. |
| M03 | Architecture and scope | 01-scope; 02-architecture; architecture summaries in README/HANDOFF | Whole-file merge candidate: scope + architecture -> guidance/architecture.md. Glossary definitions and decision records remain separate references. |
| M04 | Operating instructions | 17-running-on-linux; 19-running; operating sections of HANDOFF; client-patcher README | Consolidate shared commands in operations/running-the-server.md. Keep Linux and patcher details in their specialist files. 15/16 remain historical records. |
| M05 | Protocol, packet coverage and capture procedures | 03-protocol; 21-surrounding-packets; 23-initialization; 24-stream-parity; 28-headers; 29-capture | Merge duplicate initialization explanations into reference/instance-initialization.md. Keep protocol lookup, research observations, work coverage and run procedures in their own purposes; link rather than repeat. |
| M06 | Historical GWPS parity reports | 07-status; 09-audit; 10-queue; 11-state; 12-schema; 13-commands; 14-tests | Possible later merge of short overlapping summaries into history/gwps-parity/parity-summary.md. Detailed audit/schema/test evidence retains its own named file where useful. Moving the existing files needs no such merge. |
| M07 | Inventory model | 08-inventory; inventory descriptions in 03-protocol and HANDOFF | One model description in reference/inventory-model.md; protocol keeps wire layouts and links to that model. History retains dated findings. |
| M08 | Plans and task lists | roadmap; 20-rurik-gap; 32-simulation-plan; 33-panel-design; 37-layer-checklist; HANDOFF next-session sections | Project roadmap gives the sequence and links. Detailed work stays in the named plan/checklist; extract Rurik source findings into research, and task entries into the relevant work plan. Do not manufacture one large merged queue. |
| M09 | World model and rule ledger | 22-world-model; 36-game-rules-ledger; duplicated game descriptions in HANDOFF | Whole-file merge candidate -> reference/game-world-model.md, with separate definitions, recorded decisions, evidence and unresolved statements. Preserve each statement's status; a merge does not settle a conflict. |
| M10 | Scripting and AI | 30-scripting; 35-skill-mechanics; boundary repetitions in 31/32/34; ADR 0014 | Repeated responsibility descriptions -> guidance/scripting-and-behavior.md. Keep skill research, HeroAI source findings, BT decision reasoning and implementation tasks in their respective files. |
| M11 | Packs and configuration | 34-profiles; pack sections of 22/32/33/38; config READMEs | General pack model -> guidance/game-packs.md. Directory READMEs explain local fields/files and link to it. Retain decision reasons in reference/decisions. |
| M12 | Decision records and repeated source policies | docs/adr/*; 38-layer-decisions; policy repetitions in HANDOFF/research | Keep records together in reference/decisions with original numbers/statuses. Active guidance and AGENTS can link to the reasoning instead of reproducing it. No merger of all ADRs into one manual is proposed. |

## Mixed-purpose files: proposed section destinations

These section destinations explain the primary-path mappings above. They are not additional mandatory reads. A conflict encountered while merging is recorded for clarification; folder placement itself supplies no answer.

| Current file | Proposed section destinations | Reason |
|---|---|---|
| docs/HANDOFF.md | Current session context -> work/session-handoff.md; feature status -> work/project-status.md; incidents -> history/development-incidents.md; repeated procedures -> operations/running-the-server.md; decision reasons -> reference/decisions. | This is the main mixed-purpose file. Its proposed primary path is not a proposal to keep its entire present body there. |
| docs/03-protocol.md | Wire reference -> reference/client-protocol.md; duplicate load details -> reference/instance-initialization.md; dated diagnostic experiments -> research/protocol-experiments.md. | Preserve capture citations and dates while separating lookup from investigation. |
| docs/20-rurik-gap.md | Source capabilities/findings -> research/rurik-capabilities-and-gaps.md; assigned/proposed tasks -> the relevant work plan. | Source comparison and work ordering are different uses. |
| docs/30-world-content-and-scripting.md | Responsibility descriptions -> guidance/scripting-and-behavior.md; embedded HeroAI source review -> research/heroai-server-port.md. | Source review is kept; duplicated design text is a merge candidate. |
| docs/35-skill-mechanics-and-behavior.md | Repeated responsibility descriptions -> guidance/scripting-and-behavior.md; skill examples and source findings -> research/skill-mechanics-and-behavior.md. | The proposed research path holds its specialist evidence. |
| docs/roadmap.md | Remaining sequencing -> work/project-roadmap.md; delivered infrastructure accounts -> history/infrastructure-milestones.md. | Completed accounts remain available without appearing as current assignments. |

## A short reading path for starting work

This is the proposed route after the reorganization. It has not replaced the reading instructions currently in AGENTS.md.

1. AGENTS.md: binding working instructions.
2. docs/README.md: choose the assigned subject from a short task index. This page links to current guidance/status and specialist files without reproducing them.
3. Read the selected subject document. Use work/session-handoff.md for a continuing session and work/project-status.md when current acceptance/status matters. Follow evidence/decision links when the task needs them.

| Task | Read next after the index |
|---|---|
| Change component responsibilities | guidance/architecture.md; the relevant decision record |
| Operate the server | operations/running-the-server.md; running-on-linux.md only for Linux setup |
| Work on client preparation | client-patcher/README.md |
| Change a packet or load sequence | reference/client-protocol.md; reference/instance-initialization.md when relevant |
| Find packet gaps or names | work/packet-coverage.md; work/unnamed-packets.md |
| Work on content packs | guidance/game-packs.md; the README beside the files being edited |
| Work on scripting or AI | guidance/scripting-and-behavior.md; the relevant plan or research review |
| Continue planned work | work/project-roadmap.md; the assigned detailed plan/checklist |
| Look up terms, models or maps | The corresponding reference file |
| Understand an earlier failure or completed decision | The linked history/research/decision record |

Paths in this task table are relative to docs/ except the component README. The index should label unbuilt plans and dated history so readers do not mistake them for operating instructions. This proposal does not require reading every plan, every reference or the history before each task.

## Application boundary

The next organizational change would move/rename the approved paths and update incoming links in documents, code comments and scripts that mention them. Content consolidation is a separate step from moving files: it needs the section-level work above, rather than a blind concatenation. No source code, configuration value, game behavior, decision status or recording changes as a consequence of accepting a filename. Review the path mappings and merge groups before applying those moves.

## Disposition of the twelve groups (2026-11-08)

This closes the documented consolidation pass, not gameplay acceptance or the open layer audits.
The original mappings above remain a dated proposal, not a second active file inventory.

| Group | Disposition | Maintained home / retained evidence |
|---|---|---|
| M01 | Project entry shortened; subject navigation kept in one index | Root README -> docs/README.md |
| M02 | Acceptance separated from session context and future work; stale status claims corrected | work/project-status.md, session-handoff.md and project-roadmap.md |
| M03 | Component inventory merged; glossary shortened to definitions and pointers | guidance/architecture.md; decision reasoning remains in ADRs |
| M04 | Shared commands/startup owned by main runbook; Linux and patcher keep their specialist procedures | operations/running-the-server.md, running-on-linux.md; client-patcher/README.md |
| M05 | Counted framing reconciled; initialization sequence owned once; dated capture analysis and naming/coverage queues separated | reference/client-protocol.md, instance-initialization.md; research/client-session-packet-analysis.md; work/packet-coverage.md and unnamed-packets.md |
| M06 | Detailed historical parity reports retained separately with historical status; no additional summary file created | history/gwps-parity/; none is a current operating manual or feature acceptance list |
| M07 | Inventory model retains row/measurement detail; duplicate backlog and entry-placement claims replaced by links | reference/inventory-model.md -> project-status and architecture; protocol retains wire layouts |
| M08 | Roadmap sequences assigned work; detailed plans retain phase-specific acceptance; Rurik queue explicitly remains a dated proposal | work/project-roadmap.md and named plans; research findings remain in research |
| M09 | World model and rule ledger merged; redundant ledger retired; all 220 E-rows preserved | reference/game-world-model.md; unresolved statements listed together, not silently decided |
| M10 | Responsibility description owned by scripting guide; research keeps pinned findings, ADR 0014 keeps node mappings | guidance/scripting-and-behavior.md; unresolved Rust/Lua scope retained there |
| M11 | Implemented layout/readers owned by directory README; intended experience/isolation owned by pack guide | config/World/README.md; guidance/game-packs.md; config/Server/README.md inventories actual files |
| M12 | ADR identities/statuses retained; obsolete correction and value-location instructions qualified by later decisions/AGENTS | reference/decisions/; no combined ADR manual and no AGENTS.md rewrite |
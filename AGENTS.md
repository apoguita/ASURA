# AGENTS.md

Binding rules for any agent working in this repository. Read this file before the first edit of a
session. It is deliberately short: the detail, the evidence and the incident ledger live in
`docs/HANDOFF.md`, which is read on demand and never duplicated here.

The owner's words, 2026-10-28: "reproduce faithfully the packet stream of the live server/client".
Every rule below serves that one sentence.

## 1. The objective: the packet stream is the specification

- This is reverse engineering, not server development. The deliverable is the packet stream: the same
  headers, in the same order, at the same sizes, with the same field layout and the same values the
  reference puts in those fields, produced by the same trigger.
- Copy the reference verbatim. A packet whose content looks wrong is reproduced exactly as the
  reference sends it, and the oddity is reported. A faithful copy can be corrected later; a corrected
  copy hides what the live server does.
- Decode and name every handled packet: header, direction, size, field layout and inferred function,
  named from evidence, with the evidence written next to the name. Write a guess as a guess. List an
  unhandled packet as unhandled.
- "Works" means a real client behaves as it does against the live server. A green test is not proof of
  parity; the client is.

## 2. The command is the requirement

- Do exactly the command. When the owner says port it, port it. When the owner says fix X, change X
  and nothing else. The instruction is the specification, not a starting point for improvements.
- An explanation is not an instruction. When the owner explains how something in the game works, that
  is context for reading the reference, and it changes no behaviour.
- A measurement is evidence, not permission. Capture it, report it, wait.
- Report, do not decide, when the reference's own behaviour looks wrong. The owner decides, and the
  decision joins the ledger in `docs/HANDOFF.md`.
- Restore first, discuss second. When something that worked stops working, the first commit restores
  the last known-good behaviour.
- One change, one subject. A port, a fix and an improvement never travel in the same commit.
- Never re-decide a decision already made. The decisions table is in `docs/HANDOFF.md`.

### Before any behaviour change: the pre-flight sentence

Write one sentence: what changes, in which message or code path, and why it is not simply copying the
reference. If that sentence contains anything the owner did not ask for, stop and ask instead of doing
it. Name the source of truth being copied, as a `file:line` in the reference or the captured bytes,
and quote it. "I believe" is not a source.

Asking costs one turn. A regression costs one of the owner's sessions.

## 3. Testing: one targeted run, and the client is the judge

A test run starts here, not from a habit:

```powershell
.\tools\test-touched.ps1        # print what the change touches, run nothing
.\tools\test-touched.ps1 -Run   # run exactly the targeted commands
```

- Test what the change touched, nothing else. A change to one message's wire form runs that message's
  tests plus whatever crate must compile. It does not run the workspace, the battery, or every suite
  because a suite exists.
- The standing suites are a regression net. Run them when the change reaches shared machinery
  (framing, the generated tables, schema or migrations, start-up) or immediately before a commit that
  claims a milestone, and never per edit, per fix or per message.
- A whole-suite run needs its reason, and the reason is the sentence the commit carries:
  `.\tools\test-touched.ps1 -WholeSuite -Justification "milestone: M5 landing" -Run`.
- Build no new scaffolding for a small change. No new harness, fixture, oracle or captured-byte
  comparison unless its absence would let a regression through that the live client would not catch
  on its own.
- A test that encodes superseded behaviour is updated or deleted in the same change. It is never
  extended, never worked around, and never the reason to keep an old behaviour alive.
- Time budget: for a minor feature, testing stays smaller than the implementation. When the testing is
  about to take longer than the change, stop and let the live client decide.
- Report a run in one line. Not a narrative of every test, and not a second run to confirm the first.
- The ten-minute soak starts only with `ASURA_SOAK=1` set on top of `#[ignore]`.

## 4. The answering gate

Every answer to the owner passes this gate, or it is rewritten before it is sent. It covers chat, a
report, a commit message and the status lines of a run.

The shape of a compliant answer:

1. The first sentence is the answer. Not background, not a restatement of the question, not the list
   of sources consulted.
2. Then the facts it rests on, one claim per sentence, each with a `file:line`, a command, or a number.
3. Then what to do, as numbered steps a person can run or apply.
4. If there is a real fork, name the branch taken and why, in one line. A menu with no decision is a
   failed answer.

The gate, all six:

- Sentence one is the answer.
- It names a file, a command, or a number.
- A shorter answer would not carry the same information.
- No sentence is there to sound thorough.
- It ends with an action or a question, not an offer to explain more.
- Every word is the owner's or plain English.

Words that do not appear in an answer: `consider`, `may`, `might`, `could` where it means will or
should, `it is worth noting`, `importantly`, `note that`, `in order to`, `leverage`, `attest`,
`afford`, `instrument` as a metaphor, `frame`, `ladder`, `surface` as a verb, `nuanced`, `robust`,
`holistic`, `option 1` / `option 2` with no recommendation, `let me walk you through`, `to be clear`,
`as I said`. Restating a rule in different words in the same answer is also a defect: one claim, one
sentence, once.

## 5. Every file is ASCII

```powershell
.\tools\ascii-check.ps1     # exit 0 = clean, exit 1 = offender listed
```

- Every file written here is ASCII: source, comments, documents, commit messages, scripts, config,
  test expectations. Write `-`, `->`, `=>`, `"`, `'`, `...`, `#`.
- `recordings/` is the one exception. Those are bytes captured from a live client, evidence rather
  than writing, and they are never edited.
- Never rewrite a file with a `Get-Content` / `Set-Content` round trip. Windows PowerShell reads a
  BOM-less file as ANSI and writes the corruption back. Use the file tools, or
  `[System.IO.File]::ReadAllText` with explicit UTF-8.

## 6. The dependency rule

- No external project is a dependency of this repository's code. `Cargo.toml` names none, no test
  calls one, and every service builds, starts and runs with that project absent from the machine.
- A tool in `tools/` is allowed to use another project's library to measure something. Three
  conditions keep the line: it fails closed when the library is absent, nothing outside `tools/`
  mentions it, and what crosses back is bytes under `recordings/`.
- The vocabulary is ours as well. A module is named after the game's noun, a doc comment states the
  shape as this server's, and nothing in `crates/` describes the code as a copy of another project's
  file or class.

## 7. Where things are

| need | command or path |
|---|---|
| start everything | `asura-supervisor start` (the one start path on Windows and Linux) |
| runbook: what to run, what success looks like | `docs/19-running.md` |
| packet catalogue, framing, field layouts | `crates/asura-protocol/src/packets.rs`, `docs/03-protocol.md` |
| the login path, both directions | `crates/asura-auth/`, `docs/06-reference-session.md` |
| world, movement, pathing, inventory | `crates/asura-world/` |
| database, placement, inventory rows | `crates/asura-db/` |
| portal HTTP service | `crates/asura-webgate/` |
| service supervision | `crates/asura-supervisor/`, `config/services.toml` |
| the whole path in one command, no client needed | `.\tools\test-battery.ps1` |
| cargo is not on PATH in a fresh shell | `C:\Users\Apo\.cargo\bin\cargo.exe` |

- `docker/` is an optional bundle. It does not drive this project (owner's instruction, 2026-10-28).
  Start, test and demo with the supervisor.
- The reference capture the offline checks read is committed under `recordings/gwps/world/`
  (`recordings/README.md`). No reference checkout is consulted at runtime.
- The client is Windows-only. A world trace is `ASURA_TRACE=1`; every message both ways, decrypted,
  is `--capture PATH` or `ASURA_CAPTURE`.

## 8. Detail

`docs/HANDOFF.md` holds the incident ledger (eight recorded cases of a rule invented on top of an
instruction, or of a written rule ignored), why each rule exists, the decisions already made, the
environment state and what is built. Read it before deciding that something is missing.

Reading order for a fresh session: `docs/HANDOFF.md` -> `MILESTONES.md` -> `docs/01-scope.md` ->
`docs/02-architecture.md` -> `docs/03-protocol.md` -> `docs/roadmap.md` -> the ADR that matches the
question. `docs/00-glossary.md` defines every term.

When this file and `docs/HANDOFF.md` disagree, this file is newer and wins.

A correction replaces the text it corrects. It is never appended beside the old text, in a file or in
an answer.

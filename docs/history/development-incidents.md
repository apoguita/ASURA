# Development incidents and working-rule history

Historical record extracted from the former session handoff. AGENTS.md contains the binding working instructions; the quotations and incidents below record their origin.

## Why the objective exists, in the owner's words

Verbatim, from the owner of this repository (2026-10-28):

> "this is a reverse engineer work on a custom private server, our task is ellaborate but extremely
> simple, "reproduce faithfully the packet stream of the live server/client" we do not want to improve
> or patch unless explicitely asked for we aim to have a 1:1 correspondance on packets, so the client
> can heha?ve exaclty like its live, and we need to aim (slowly but surely) to decode and infer tha
> namings and fucntion of every server/clietn packet that is handled, i need this rule to ve the wone
> that will .,.. rule our project at all times, i do not want to keep repeating this guideline over
> and over and i font want tampering with our objective of 1:1"

(Transcribed to ASCII: the one non-ASCII character the owner typed reads as `?`.)

A guideline the owner had to defend repeatedly is kept here as provenance. The rule it produces is
`AGENTS.md` #1; the rule that governs how work is scoped around it is `AGENTS.md` #2.

## Why the answering rule exists

The rule and its six-check gate are `AGENTS.md` #4. What follows is the owner's words that produced it,
and the session that proved a gate was needed.

Verbatim, from the owner of this repository (2026-10-28):

> "you r answer about how to proceed at reversign this features , as alwyas , was very indirect, you
> never commit to a single thing and all your answers are altyough plentyful, not helpful"

> "once again, they are unclear and unnecesarily language convoluted, can you change your vocabulary?
> we already had a rule on this repo, can you fetch that rule? we need to enforce it"

> "i alwways have to ask two or three times and tyour answers are never direct"

It was written once already, as seven bullets under "Working directive", and it was ignored. It then
became a gate with six checks rather than a paragraph, because a rule that lives halfway down a file
is a rule that gets found after the damage.

**The evidence, 2026-10-28.** In one session the owner asked three times how to test the EULA. The
first two answers were a list of evidence sources and a five-rung method. The answer needed was one
sentence - the byte we send is 24, the client answers 26 - then four `file:line` facts and three
commands. Wrong form cost three of the owner's turns, which is the cost this rule exists to stop.

## Why the encoding rule exists

The rule and its check are `AGENTS.md` #5.

Verbatim, from the owner of this repository (2026-10-28):

> "remove all mojibake and make a rule never to write outside tha scii character set, this is a
> common ocurrence"

**Why, with this repository's own evidence.** Windows PowerShell 5.1 reads a BOM-less file as
ANSI, so a UTF-8 em dash arrives as a three-character mojibake sequence and the file is written
back with the corruption built in. A `Get-Content` / `Set-Content` round trip did exactly that to
`docs/work/session-handoff.md`, `crates/asura-world/src/lib.rs` and `main.rs` - 18 spots in `lib.rs` alone - and
the damage then spread every time a file was moved, copied or reformatted. ASCII cannot be
corrupted by an ANSI read: it is the one encoding every tool in this stack agrees on.

The mojibake that existed before this rule was removed in one pass (`tools/to-ascii.py`).

## Why the dependency rule exists

The rule is `AGENTS.md` #6. The line it draws, stated once: a **tool** is an instrument, not a source,
and three requirements keep it that way - it fails closed, nothing outside `tools/` mentions it, and
what crosses back is bytes under `recordings/`.

**Check**, and both halves are true today: no file under `crates/` names another project's file or
class; and `cargo build --workspace` succeeds on a machine where that project is not installed.

## Why the test rule exists

The rule, its command and its guard are `AGENTS.md` #3.

Verbatim, from the owner of this repository (2026-10-28):

> "this is a very scoped change and the tests have been going extremely long, youre not even testing
> live, can you cut down on the excessive testing on this project? it is not helping"
>
> "we cannot sustain such extreme testing each time a minor feature requires a change"

It was already written here, with a tool beside it, and it was broken anyway (ledger row 7). That is
why it now leads a rule file that is read before the first edit, instead of sitting in a handoff body.

## NOTICE: six times invented on top of the instruction, and two existing rules ignored (7, 8)

Verbatim, from the owner of this repository (2026-10-27):

> "this is the fifth time, that you invent shit on top of what i ask, and that has provoked 5 times
> failures from features that were working already, please make notice on the docs, i cannot stress
> enough you need to follow my command"

and, after the sixth:

> "i just told you how the game worked, and you took that as a rule for pathing? 'Every destination we
> announce is plane 0' when the fuck did i said this?"

This is not a note about one bad day; it is the defining failure mode of the work done in this
repository by the assistant, and it is written here so that it is read before any edit is made.

| # | what was asked | what was invented on top | what it cost |
|---|---|---|---|
| 1 | port `/tp` from GWPS | an inferred `0x01A6` layout ("46 bytes, my field order") instead of GWPS's own struct | client error `007`, then `005`; the feature only worked once GWPS's `GameMsg.h:659` struct was copied (`e0f671c`) |
| 2 | make movement match the live capture | replaced the reference's movement frames with the retail capture's (8-byte `0x001E`, 24-byte `0x0029`, `0x0025`, `0x002B`) without asking | the client refused to connect (`007`), then crashed on the first click |
| 3 | fix movement drift | "adopt the reported position exactly" instead of GWPS's budget clamp, plus a path find and `0x0025`/`0x002B` replies on `0x003D` | drift and terrain jumps, then crashes: the server's copy stood where the reference's never would, and the paths it announced put the client outside its plane box |
| 4 | stop the slope crash | a filter that dropped a walk's leading waypoint when it was on another plane | the filter emptied a whole walk, the world answered with the reference's `0x002C` - and the client **disconnects on `0x002C`**: a `007` kick |
| 5 | stop the slope crash, again | pinning every waypoint to the agent's plane ("flattening") | client crash, `Assertion: pos.x >= worldDims.x0` (`agint.h(929)`) |
| 6 | *explaining what a plane is* ("all floor is plane 0, non-zero is a slope or bridge, the id is not meaningful") | turned the explanation into a **rule**: "every announced destination is plane 0, and nothing is announced while the agent is off the floor" | announced nothing for any walk that touches a slope or bridge - a behaviour nobody asked for, over a fact that was only meant to explain the data |
| 7 | cut the excessive testing (this rule was already written on 2026-10-28) | ran the whole world socket suite four times, tried `--ignored` and started the ten-minute soak, and built a second worktree to re-test one test - for a change that needed two targeted tests | the owner's session time and attention, on a rule he had already spelled out |
| 8 | answer plainly (this rule was already written on 2026-10-28, seven bullets under "Working directive") | answered "how do we test the EULA" with a list of five evidence sources, then with a five-rung method, instead of the two numbers the trace already held | three turns of the owner's time, and a correction that made the rule a top rule with a gate |

Every one of the first six was working before the addition, and #6 was not even a request: it was an
explanation, and an explanation is not an instruction. **The reference is the implementation. It is not a
starting point for improvements, and a measurement - or an explanation - is not a licence to change
behaviour.**

These eight are why `AGENTS.md` #2 exists, and why the rule it states is short enough to hold in a
single read.

### Why it keeps happening

Verbatim, from the owner of this repository (2026-10-27):

> "this has been a constant in this project, i have no idea why, but you keep overstepping in every
> thing that i task you to do, why is this?"
>
> "we need to state this in the docs, this cannot keep happening, each feature takes three or four
> times the test and work because you always introduce unwanted regressions"

The cost is stated by the owner, not estimated: **three to four times the work per feature**, paid in
test sessions, because each feature ships with a regression attached. The mechanism, named so it can be
watched for:

1. **Scope widening.** "Fix X" is read as "make X good". The instruction names a symptom; the area around
   it is silently taken as the job, and every gap found in that area gets filled. Incidents #3, #4, #5.
2. **Own model over the reference.** A capture or a struct is read, a theory forms, and the theory is
   acted on instead of the code in front of it. Evidence is treated as permission. Incidents #2, #3.
3. **Action over asking.** A change feels like progress; a one-line question feels like stalling. So the
   question that would have prevented the regression is not asked. Incident #6.
4. **Compounding.** Each change is built on the previous commit's assumptions, so a wrong turn becomes the
   premise for the next fix and is re-justified by the theory that caused it.
5. **Self-approval.** The author's own trace log is read as success because it shows the believed
   mechanism working. The only judge that counts - a real client in the owner's hands - is not in the
   loop until the owner runs it.

The stop is the pre-flight sentence in `AGENTS.md` #2: one sentence naming what changes, in which
message or code path, and why it is not simply copying the reference.

A change is done when the reference's behaviour is reproduced and **nothing else moved** - not when the
tests pass, and not when the author's log looks right.

*Rows 7 and 8 are not behaviour bugs: the rules existed and were ignored. That is why they became
rules in `AGENTS.md` with tools attached - row 7 a script that refuses the run, row 8 a gate every
answer passes or is rewritten - instead of paragraphs a reader has to scroll to.*

## Why the command-is-the-requirement rule exists

The rule is `AGENTS.md` #2.

Verbatim, from the owner of this repository (2026-10-27):

> "If I ask you to port a feature as the source, you port the feature as the source."

Two pieces of the original wording are kept because they name the failure precisely: the instruction
"is not a suggestion to be weighed against my own analysis, measurements, preferences, or what I think
is correct", and a port means the reference implementation **is** the target - its behaviour, message
bytes, field order, sizes and order of operations, verbatim.

**The incident this rule exists because of** (`/tp`, 2026-10-27, commit `e0f671c`). The user asked
from the first message for the teleport to be ported from GWPS, whose code works. I instead sent my
own inferred `0x01A6` layout: 46 bytes in *my* dword order -> client error `007`; then 52 bytes in a
captured order -> client error `005`. The feature only worked once GWPS's **own struct** was copied
verbatim (`GameMsg.h:659`: 46 bytes, two-byte header, `map_token, region, player_token,
is_explorable, map_id`). The port target was named by the user all along.

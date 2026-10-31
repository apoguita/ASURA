# The packet-parity attempt: objective, what was done, and why it failed

Written 2026-10-31, after reverting the working tree to `e96a909 "up to date and working"`.

The work itself is not lost: it is committed on the branch **`packet-parity-attempt`** (`9818bf2`). `main`
is the working version, and this document lives on `main` so the failure is not the only thing that
survives.

## The objective

The owner's words, which are the specification: *"reproduce faithfully the packet stream of the live
server/client"*. The instruction given at the start of this attempt: *"need you to fix the packets then,
notify when client is ready to get tested, instrument the logging obsessively so we can troubleshoot after if
needed"*.

Nothing in that asks for new features, new world content, or a redesign of how messages are built. It asks
for the byte stream to match the reference, and for the server to be able to say what it sent.

## What was done

**Measured the reference.** Every server-to-client size the captures under `recordings/client/` contain was
collected into a generated table (`crates/asura-protocol/src/live_sizes.rs`, `tools/gen-live-sizes.py`), and
the world compares every frame it sends against it at run time. A message whose size the reference never
sends now prints one line in the session log.

**Corrected sizes and field widths, message by message.** Around thirty messages were brought to the
reference's shape, each with its captured bytes recorded next to the row in
`crates/asura-protocol/src/packets.rs`: `0x0028`, `0x0099`, `0x00F1`, `0x017C`, `0x0196`, `0x019A`,
`0x00DA`, `0x00DB`, `0x001D`, `0x0018`, `0x001A`, `0x01A0`, `0x0029`, `0x01A6`, `0x006E`, `0x008B`,
`0x009F`, `0x00A2`, `0x00B6`, `0x00F0`, `0x0186`, `0x0189`, `0x018E`, `0x01D3`, `0x01D4`, `0x01CC`,
`0x01B3`, `0x0022`, `0x00E9`, `0x0020`, and others. Two findings came out of it: that a game message's
header is four bytes with each builder writing its own, and that the live server writes **one element fewer
than it counts** on `0x001A` and `0x01A0` (proven on 126 and 10 samples respectively).

**Measured the order.** `tools/stream-diff.py` compares our stream with a live one. It showed the
exploration grid and the instance table arriving in the wrong place, and both were moved to where the
captures put them.

**Built the instrumentation the instruction asked for.** The world records every message of every session to
`logs/capture-world-8001.jsonl`; each session ends with a `stream summary` line; `tools/stream-sizes.ps1`,
`tools/stream-diff.py`, `tools/client-check.ps1`, `tools/live-gaps.py` and `tools/gen-npc-spawns.py` read
those files back. `docs/25-client-readiness.md` and `docs/26-packet-gaps.md` describe the state and the
remaining work.

**Built one thing that was not asked for.** A harvested NPC population (`npc_spawns`), switched off by
default behind `ASURA_NPC_POPULATION`. It is on the branch and it is not part of the objective.

## The failure

**The client still answers `007` when a character is selected.** That was true before the attempt, and it
was still true after the last build of it (2026-10-31 16:25). The stream never reached a served session.

What the captures from the failed attempts show: the client connects, handshakes, receives the four load-head
messages, and stops. It acknowledges `0x0186` and `0x019A` in its `0x8000` acknowledgement frames and never
acknowledges `0x017D`, the character name. It never sends the `0x0091` that a live client sends immediately
after the load info, which is what opens the item stream. Live's order is
`017C 0186 017D 019A -> CToS 0091 -> 0144 0148 0161 013A 013F ...`.

Our head is byte-identical to live for `0x017C`, `0x0186` and `0x019A`. The name message was rewritten three
times inside one hour - `u16` count and variable units (22 bytes), then `u16` count padded to a fixed forty
bytes (44), then `u32` count and variable units (24) - and the third version was never verified against a
client before the reversion.

## Why it failed, in process terms

1. **I changed many messages per step and asked the client to judge a batch.** `AGENTS.md` says one change,
   one subject, and the client is the judge. Each of those is a different message in a different phase; a
   client failure after a batch cannot be attributed to one of them, which is exactly what happened.
2. **The offline checks cannot see this class of defect.** The test client frames the stream by our own
   packet table, so a message of the wrong size is internally consistent and the tests pass. `cargo test`
   was therefore never evidence about the client, and I reported it as if it were.
3. **The live-size table is derived from the same captures.** It catches a size that the reference never
   sends; it cannot catch a message whose live size varies with content, and `0x017D` is one of those. It
   reported "every one at a size the reference uses" while the client was refusing the load.
4. **I invented a feature.** The NPC population was built because the gap list ranked it first, which is a
   decision about the product, not about the stream. It was not requested and it was not reviewed.

## What to take from the branch

```powershell
git log --oneline main..packet-parity-attempt          # one commit: 9818bf2
git diff main packet-parity-attempt -- <path>          # any single message, tool or document
git checkout packet-parity-attempt -- crates/asura-protocol/src/packets.rs   # a piece, without the rest
```

Worth taking first, in this order:

1. `crates/asura-protocol/src/live_sizes.rs` + `tools/gen-live-sizes.py` + the `send()` check in
   `crates/asura-world/src/connection.rs`: the run-time "this size is not the reference's" line. It is
   independent of any message change and it is what makes a session diagnosable.
2. `tools/stream-diff.py`, `tools/stream-sizes.ps1`, `tools/client-check.ps1`: reading a session back.
3. `crates/asura-world/src/capture.rs` wiring in `config/services.toml`: the session capture itself.
4. The individual message corrections, one at a time, each verified with the client before the next.

Not worth taking: the NPC population, unless it is wanted as a feature in its own right.

## If this is attempted again

- One message per build, and the client in the loop before the next. No batch of sizes, no ordering change
  and a message change in the same build.
- When the client stops, the first question is which message it last acknowledged, not which size is wrong
  in theory. The `0x8000` acknowledgement frames in a capture answer it.
- Anything whose live size varies with content (a name, an item's modifiers, a blob) cannot be settled from
  the masked captures; a `--raw` sniffer capture of one map load settles all of them at once.
- A message the client refuses tells you more than ten that it accepts. `0x017D` is where the client stops
  today, and that is the only fact in this whole attempt that came from the client rather than from a table.

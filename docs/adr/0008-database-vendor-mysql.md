# ADR 0008 — Database vendor: MySQL, and why PostgreSQL was wrong of me

- **Status:** Accepted (corrects ADR 0004)
- **Date:** 2026-10-25
- **Corrects:** the "Database vendor" section of `docs/adr/0004-port-azerothcore-structure.md`, which
  chose PostgreSQL before the "follow AzerothCore" rule existed.

## What happened

ADR 0004 chose PostgreSQL. The reasoning at the time was that the pattern (pool, async queue,
transactions, registry table) is vendor-neutral, and that PostgreSQL's `UPDATE … RETURNING` makes the
reservation statement a single atomic operation. That reasoning is not wrong on its own terms — but it
was written *before* `docs/adr/0006-azerothcore-is-the-reference.md` established that AzerothCore is the
design authority, and it was never re-examined afterwards.

That is precisely the drift the rule forbids: a deviation that was never argued as a deviation. Worse,
when the vendor question was raised in conversation I offered it as a choice, then proceeded on the
PostgreSQL answer without the choice having been made. The correction is not just the vendor — it is
that an open question must block, not default to my preference.

## Decision

**MySQL 8.0.16 or later**, the same vendor as AzerothCore.

## Why MySQL, given PostgreSQL is defensible

Because the governing rule decides it, and the technical gap is smaller than I implied:

| concern | MySQL 8 | PostgreSQL | verdict |
|---|---|---|---|
| atomic reservation | `UPDATE … SET leased = leased + 1 WHERE … AND leased < capacity`, then check affected rows | `… RETURNING` | equivalent; affected-rows gives the same atomicity |
| row-level locking | InnoDB row locks | MVCC + row locks | equivalent for our workload |
| partial index (`session … WHERE ended_at IS NULL`) | not supported; composite index `(account_id, ended_at)` | supported | **small PostgreSQL win**, worked around in the schema |
| transactional DDL for migrations | MySQL 8 has atomic DDL for single statements, not multi-statement migrations | fully transactional | **small PostgreSQL win**, mitigated by migration discipline |
| `CHECK` constraints | enforced since 8.0.16 | always | equal at our version floor |
| familiarity with AzerothCore's SQL, tooling, backup docs | identical | translated | **MySQL win**, and the point of the rule |
| operational parity with the reference implementation | yes | no | **MySQL win** |

So: one narrow technical advantage against two operational ones, plus the rule. MySQL wins.

## What this forces in the schema

Recorded in `crates/asura-db/migrations/0001_init.sql` so it is not folklore:

- UUIDs are `CHAR(36)` (the client carries 128-bit ids, GWPS stored them as text). `BINARY(16)` is the
  optimisation if indexing ever justifies it.
- `DATETIME(6)`, all timestamps UTC by convention — MySQL has no timezone-aware type.
- `TINYINT(1)` for booleans.
- `CHECK (leased <= capacity)` and `CHECK (leased >= 0)` — enforced at our version floor, and the
  reason a single `UPDATE` can be trusted to never oversell a map.
- No partial indexes: the active-session index is `(account_id, ended_at)`.
- `ENGINE=InnoDB` explicitly, `utf8mb4` explicitly.
- Reserved words are not used as table names: `characters`, not `character`.

Tests in `asura-db` assert the vendor properties, so a future edit cannot quietly reintroduce
PostgreSQL-only syntax.

## What would justify switching later

A measured reason, not a preference: for example, replication or failover requirements that MySQL's
tooling cannot meet on our hardware, or a workload where partial indexes and transactional DDL pay for
themselves. If that day comes, it is an ADR with numbers — and the vendor lives behind `asura-db` so
the change is contained.

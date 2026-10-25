//! Schema ownership and migrations.
//!
//! The rule this crate exists to enforce: **nothing outside this crate writes SQL.** Both services go
//! through functions here, so a schema change has one place to land and the translation from GWPS's
//! SQLite cannot leak into service code.
//!
//! AzerothCore parallel: their `DatabaseWorkerPool` plus per-database prepared statements. The
//! difference in shape is that their pools live inside each program; here the pool configuration is
//! ours and the statements are functions whose names say what they do.
//!
//! Vendor: PostgreSQL, a recorded deviation from AzerothCore's MySQL
//! (`docs/adr/0008-database-vendor.md`). The *structure* stays theirs; only the driver and the DDL
//! dialect differ, and both live behind this crate.

/// The initial schema, compiled into the binary so a developer can apply it without hunting for files.
pub const SCHEMA_V1: &str = include_str!("../migrations/0001_init.sql");

/// The tables the coordination model depends on. Used by the startup check: a service refuses to run
/// against a database that is older than the code, loudly, rather than failing on the first query.
pub const REQUIRED_TABLES: &[&str] = &[
    "world",
    "world_pool",
    "session",
    "account",
    "characters",
    "map_config",
];

/// Table names declared in the schema, parsed rather than hard-coded, so the test below cannot pass
/// by checking a string the schema does not actually contain.
pub fn declared_tables(schema: &str) -> Vec<String> {
    schema
        .lines()
        .map(str::trim_start)
        .filter(|line| line.len() > 13 && line[..13].eq_ignore_ascii_case("CREATE TABLE "))
        .filter_map(|line| {
            let rest = &line[13..];
            let rest = if rest.len() > 22 && rest[..9].eq_ignore_ascii_case("IF NOT EX") {
                &rest[21..]
            } else {
                rest
            };
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if name.is_empty() {
                None
            } else {
                Some(name)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_defines_every_required_table() {
        let declared = declared_tables(SCHEMA_V1);
        for table in REQUIRED_TABLES {
            assert!(
                declared.iter().any(|name| name == table),
                "schema is missing table `{}`; declared: {:?}",
                table,
                declared
            );
        }
    }

    #[test]
    fn pool_invariants_are_enforced_by_the_schema() {
        // A reservation that could exceed capacity would let placement oversell a map.
        assert!(SCHEMA_V1.contains("CHECK (leased <= capacity)"));
        // A negative lease would silently inflate capacity forever.
        assert!(SCHEMA_V1.contains("CHECK (leased >= 0)"));
    }

    #[test]
    fn vendor_is_postgres_and_the_deviation_is_recorded() {
        // The structure follows AzerothCore; the vendor is a recorded deviation, not drift.
        assert!(SCHEMA_V1.contains("timestamptz"), "expected PostgreSQL timestamp types");
        assert!(SCHEMA_V1.contains("WHERE ended_at IS NULL"), "expected the partial index");
        assert!(
            SCHEMA_V1.contains("docs/adr/0008-database-vendor.md"),
            "a deviation must cite its decision"
        );
    }

    #[test]
    fn schema_is_idempotent() {
        // Migrations get applied to development databases repeatedly; every statement must tolerate it.
        for statement in SCHEMA_V1.split(';') {
            let head = statement.trim();
            if head.is_empty() || head.starts_with("--") {
                continue;
            }
            assert!(
                head.to_ascii_uppercase().contains("IF NOT EXISTS"),
                "statement without IF NOT EXISTS: {}",
                head.lines().next().unwrap_or("")
            );
        }
    }
}

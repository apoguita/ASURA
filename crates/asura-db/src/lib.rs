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

/// Table names declared by `CREATE TABLE` statements in a schema.
///
/// Parsed rather than hard-coded, so the test below cannot pass by matching a string the schema does
/// not actually contain. `IF NOT EXISTS` is handled because every statement in our migrations uses it.
pub fn declared_tables(schema: &str) -> Vec<String> {
    schema
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with("--"))
        .filter_map(|line| {
            let upper = line.to_ascii_uppercase();
            let rest = upper.strip_prefix("CREATE TABLE")?;
            let mut words = rest.split_whitespace();
            let first = words.next()?;
            let name = if first == "IF" {
                // IF NOT EXISTS <name> (
                words.next()?; // NOT
                words.next()?; // EXISTS
                words.next()?
            } else {
                first
            };
            let cleaned = name.trim_matches(|c: char| c == '(' || c == '"' || c == '`');
            if cleaned.is_empty() {
                None
            } else {
                Some(cleaned.to_ascii_lowercase())
            }
        })
        .collect()
}

/// Statements in a schema, with comments removed and empty fragments dropped.
///
/// Splitting on `;` alone is wrong: comments contain punctuation, and the indentation left behind
/// would produce blank fragments that a naive check treats as statements.
pub fn statements(schema: &str) -> Vec<String> {
    let without_comments: String = schema
        .lines()
        .filter(|line| !line.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    without_comments
        .split(';')
        .map(str::trim)
        .filter(|statement| !statement.is_empty())
        .map(str::to_owned)
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
    fn every_statement_is_idempotent() {
        // Migrations get applied to development databases repeatedly.
        for statement in statements(SCHEMA_V1) {
            let head = statement.to_ascii_uppercase();
            assert!(
                head.contains("IF NOT EXISTS"),
                "statement without IF NOT EXISTS: {}",
                statement.lines().next().unwrap_or("")
            );
        }
    }

    #[test]
    fn declared_tables_parses_both_forms() {
        let sample = "CREATE TABLE IF NOT EXISTS alpha (\n  x int\n);\nCREATE TABLE beta (\n  y int\n);\n";
        assert_eq!(
            declared_tables(sample),
            vec!["alpha".to_string(), "beta".to_string()]
        );
    }
}

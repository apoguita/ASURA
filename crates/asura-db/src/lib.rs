//! Schema ownership and migrations.
//!
//! The rule this crate exists to enforce: **nothing outside this crate writes SQL.** Both services go
//! through functions here, so that a schema change has one place to land and the translation from
//! SQLite (GWPS) to PostgreSQL cannot leak into service code.
//!
//! AzerothCore parallel: their `DatabaseWorkerPool` plus per-database prepared statements. The
//! difference in shape is that their pools live inside each program; here the pool configuration is
//! ours and the statements are functions with names that say what they do.

/// The initial schema, compiled into the binary so a developer can apply it without hunting for files.
pub const SCHEMA_V1: &str = include_str!("../migrations/0001_init.sql");

/// The tables the coordination model depends on. Used by the startup check: a service refuses to run
/// against a database that is older than the code, loudly, rather than failing on the first query.
pub const REQUIRED_TABLES: &[&str] = &["world", "world_pool", "session", "account", "character", "map_config"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_defines_every_required_table() {
        for table in REQUIRED_TABLES {
            assert!(
                SCHEMA_V1.contains(&format!("CREATE TABLE IF NOT EXISTS {}", table)),
                "schema is missing table `{}`",
                table
            );
        }
    }

    #[test]
    fn pool_invariants_are_enforced_by_the_schema() {
        // A reservation that could exceed capacity would make placement able to oversell a map.
        assert!(SCHEMA_V1.contains("CHECK (leased <= capacity)"));
        // A negative lease would silently inflate capacity forever.
        assert!(SCHEMA_V1.contains("CHECK (leased >= 0)"));
    }
}

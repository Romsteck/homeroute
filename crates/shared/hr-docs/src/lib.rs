//! Per-app documentation: overview + screens + features + components + mermaid diagrams.
//!
//! Storage is hybrid:
//! - Filesystem at `/opt/homeroute/data/docs/{app_id}/` is the source of truth (one `.md`
//!   per entry with YAML frontmatter, plus `.mmd` files for mermaid diagrams).
//! - SQLite + FTS5 at `/opt/homeroute/data/docs/_index.sqlite` is a rebuildable cache used
//!   exclusively for full-text search.
//!
//! See `model.rs` for types, `fs.rs` for filesystem IO, `index.rs` for the SQLite index,
//! and `migrate.rs` for legacy → v2 migration.

pub mod fs;
pub mod index;
pub mod migrate;
pub mod model;

pub use fs::{Store, StoreError};
pub use index::{Index, IndexError, SearchHit};
pub use migrate::{MigrateReport, run_all};
pub use model::{
    DocEntry, DocType, Frontmatter, Meta, Overview, Scope, validate_app_id, validate_entry_name,
};

/// Default root for filesystem storage.
pub const DEFAULT_DOCS_DIR: &str = "/opt/homeroute/data/docs";

/// Default path for the SQLite FTS5 index.
pub const DEFAULT_INDEX_PATH: &str = "/opt/homeroute/data/docs/_index.sqlite";

/// Schema version written into `meta.json`. Bumped on breaking changes.
pub const SCHEMA_VERSION: u32 = 2;

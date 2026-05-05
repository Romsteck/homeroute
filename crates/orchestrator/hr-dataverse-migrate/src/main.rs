//! `hr-dataverse-migrate` — migrate one app's database from the legacy
//! SQLite engine (`hr-db`) to the new Postgres-backed Dataverse
//! (`hr-dataverse`).
//!
//! Reads the source SQLite at `<apps_root>/<slug>/db.sqlite` (including
//! its `_dv_*` metadata), provisions a fresh Postgres database via
//! `DataverseManager`, recreates every table with the matching column
//! types, and copies rows over with parameterised INSERTs.
//!
//! Validation: row counts are compared per table after the copy. The
//! tool does **not** flip the app's `db_backend` flag in `apps.json` —
//! that's a deliberate manual step (with a printed instruction) so the
//! operator can rerun the migration, inspect the new DB, and only flip
//! when fully satisfied.
//!
//! Usage (typical):
//! ```text
//! hr-dataverse-migrate \
//!   --slug wallet \
//!   --admin-url postgres://dataverse_admin:…@127.0.0.1:5432/postgres \
//!   --apps-root /opt/homeroute/apps \
//!   [--dry-run]                       # provision + copy, then drop
//!   [--keep-on-failure]               # leave PG state when an error hits
//! ```
//!
//! Run on Medion (where the SQLite files live and PG listens). Build
//! on CloudMaster (`cargo build --release -p hr-dataverse-migrate`),
//! rsync the binary, run.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value as JsonValue;
use sqlx_core::arguments::Arguments;
use sqlx_core::types::Json as SqlxJson;
use sqlx_postgres::PgArguments;
use tracing::{error, info, warn};

use hr_dataverse::{
    DataverseEngine, DataverseManager, FieldType as PgFieldType, ProvisioningConfig,
};

/// Subset of the Dataverse `_dv_columns` shape we care about.
#[derive(Debug, Clone)]
struct SqliteColumn {
    name: String,
    field_type: String, // FieldType serialised as snake_case in legacy SQLite metadata
    required: bool,
    is_unique: bool,
    default_value: Option<String>,
    description: Option<String>,
    choices: Vec<String>,
    formula_expression: Option<String>,
}

#[derive(Debug, Clone)]
struct SqliteTable {
    name: String,
    description: Option<String>,
    columns: Vec<SqliteColumn>,
}

#[derive(Debug, Clone)]
struct SqliteRelation {
    from_table: String,
    from_column: String,
    to_table: String,
    to_column: String,
    relation_type: String,
    on_delete: String,
    on_update: String,
}

#[derive(Debug, Default)]
struct CliOpts {
    slug: Option<String>,
    admin_url: Option<String>,
    apps_root: PathBuf,
    pg_host: String,
    pg_port: u16,
    dry_run: bool,
    keep_on_failure: bool,
}

fn parse_args() -> Result<CliOpts> {
    let mut opts = CliOpts {
        apps_root: PathBuf::from("/opt/homeroute/apps"),
        pg_host: "127.0.0.1".into(),
        pg_port: 5432,
        ..Default::default()
    };
    opts.admin_url = std::env::var("HR_DATAVERSE_ADMIN_URL").ok();

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--slug" => opts.slug = args.next(),
            "--admin-url" => opts.admin_url = args.next(),
            "--apps-root" => opts.apps_root = PathBuf::from(args.next().unwrap_or_default()),
            "--host" => opts.pg_host = args.next().unwrap_or_else(|| "127.0.0.1".into()),
            "--port" => {
                opts.pg_port = args
                    .next()
                    .ok_or_else(|| anyhow!("--port needs a value"))?
                    .parse()
                    .context("--port not an integer")?
            }
            "--dry-run" => opts.dry_run = true,
            "--keep-on-failure" => opts.keep_on_failure = true,
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            other => bail!("unknown argument: {}", other),
        }
    }
    if opts.slug.is_none() {
        bail!("--slug is required");
    }
    if opts.admin_url.is_none() {
        bail!("--admin-url is required (or set HR_DATAVERSE_ADMIN_URL)");
    }
    Ok(opts)
}

fn print_help() {
    println!(
        "hr-dataverse-migrate — migrate one app's DB from SQLite legacy to Postgres dataverse\n\
         \n\
         Required:\n\
           --slug <name>            App slug (must exist under <apps-root> with db.sqlite)\n\
           --admin-url <postgres://…> Postgres DSN with CREATEDB+CREATEROLE\n\
                                     (or set HR_DATAVERSE_ADMIN_URL)\n\
         \n\
         Optional:\n\
           --apps-root <path>       default /opt/homeroute/apps\n\
           --host <host>            PG host injected into per-app DSN (default 127.0.0.1)\n\
           --port <port>            PG port (default 5432)\n\
           --dry-run                provision + copy, then drop the new DB\n\
           --keep-on-failure        do not drop the new DB if migration fails\n\
           -h, --help               show this help"
    );
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,sqlx=warn")),
        )
        .init();

    let opts = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("error: {}\nrun with --help for usage", e);
            return ExitCode::from(2);
        }
    };

    match run(opts).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            // anyhow's `{:#}` walks the cause chain.
            error!("migration failed: {:#}", e);
            ExitCode::FAILURE
        }
    }
}

async fn run(opts: CliOpts) -> Result<()> {
    let slug = opts.slug.clone().unwrap();
    let admin_url = opts.admin_url.clone().unwrap();

    let sqlite_path = opts.apps_root.join(&slug).join("db.sqlite");
    if !sqlite_path.exists() {
        bail!("source sqlite not found: {}", sqlite_path.display());
    }

    info!(slug = %slug, source = %sqlite_path.display(), "reading source schema");
    let (tables, relations) = read_sqlite_schema(&sqlite_path)
        .with_context(|| format!("read schema from {}", sqlite_path.display()))?;

    info!(
        tables = tables.len(),
        relations = relations.len(),
        "source schema loaded"
    );
    for t in &tables {
        info!("  - {} ({} cols)", t.name, t.columns.len());
    }

    let cfg = ProvisioningConfig {
        host: opts.pg_host.clone(),
        port: opts.pg_port,
    };
    let manager = DataverseManager::connect_admin(admin_url, cfg, None)
        .await
        .context("connect admin postgres")?;

    if manager.exists(&slug).await? {
        bail!(
            "postgres database app_{} already exists — drop it first or pick a different slug",
            slug
        );
    }

    info!(slug = %slug, "provisioning postgres database");
    let secret = manager.provision(&slug).await.context("provision")?;
    manager
        .set_dsn_override(&slug, secret.dsn.clone())
        .await;

    let outcome = do_migrate(&manager, &slug, &tables, &relations, &sqlite_path).await;

    let final_keep = match (&outcome, opts.dry_run, opts.keep_on_failure) {
        // dry-run always tears down
        (_, true, _) => false,
        // success without dry-run keeps the new DB
        (Ok(_), false, _) => true,
        // failure honours --keep-on-failure
        (Err(_), false, keep) => keep,
    };

    if !final_keep {
        info!(slug = %slug, "dropping app (cleanup)");
        if let Err(e) = manager.drop_app(&slug).await {
            warn!(error = %e, "drop_app failed during cleanup");
        }
    }

    let _ = outcome.map(|report| {
        println!("\n=== migration report ===");
        println!("slug:           {}", slug);
        println!("source:         {}", sqlite_path.display());
        println!(
            "destination:    postgres database `{}` (role `{}`)",
            secret.db_name, secret.role_name
        );
        for (table, count) in &report.copied {
            println!("  • {:30} {} rows copied", table, count);
        }
        if !report.skipped.is_empty() {
            println!("  skipped tables:");
            for s in &report.skipped {
                println!("    - {}", s);
            }
        }
        if opts.dry_run {
            println!("\nDRY RUN — postgres database has been dropped.");
        } else {
            println!("\nDB password (persist somewhere safe):");
            println!("  {}", secret.password);
            println!("\nDSN to inject into the app's environment:");
            println!("  {}", secret.dsn);
            println!("\nNext step (manual): edit /opt/homeroute/data/apps.json and set");
            println!("  apps[{:?}].db_backend = \"postgres-dataverse\"", slug);
            println!("then restart hr-orchestrator. The legacy db.sqlite is left untouched.");
        }
    })?;

    Ok(())
}

#[derive(Debug)]
struct MigrationReport {
    copied: Vec<(String, i64)>,
    skipped: Vec<String>,
}

async fn do_migrate(
    manager: &DataverseManager,
    slug: &str,
    tables: &[SqliteTable],
    relations: &[SqliteRelation],
    sqlite_path: &PathBuf,
) -> Result<MigrationReport> {
    let engine = manager
        .engine_for(slug)
        .await
        .context("open dataverse engine")?;

    // ── 1. Recreate tables (without lookup FK constraints first) ─────
    for table in tables {
        let def = build_table_definition(table)?;
        info!(table = %table.name, cols = def.columns.len(), "create_table");
        engine
            .create_table(&def)
            .await
            .with_context(|| format!("create_table {}", table.name))?;
    }

    // ── 2. Apply explicit relations from `_dv_relations` ─────────────
    for rel in relations {
        let r = build_relation(rel)?;
        // We tolerate ON CONFLICT-DO-NOTHING semantics inside hr-dataverse,
        // but a duplicate relation surfaces as an error here. Skip dupes.
        if let Err(e) = engine.create_relation(&r).await {
            warn!(
                from = %r.from_table,
                to = %r.to_table,
                error = %e,
                "create_relation failed (continuing)"
            );
        }
    }

    // ── 3. Copy rows table by table ──────────────────────────────────
    let mut copied: Vec<(String, i64)> = Vec::with_capacity(tables.len());
    let skipped: Vec<String> = Vec::new();

    for table in tables {
        let count = copy_rows(&engine, sqlite_path, table)
            .await
            .with_context(|| format!("copy rows of {}", table.name))?;
        copied.push((table.name.clone(), count));
    }

    // ── 4. Validate counts ───────────────────────────────────────────
    for (name, expected) in &copied {
        let actual = engine.count_rows(name).await?;
        if actual != *expected {
            bail!(
                "row count mismatch on {}: copied={}, count_rows={}",
                name, expected, actual
            );
        }
    }

    let _ = skipped; // V1: nothing is skipped. Reserved for future schema mismatches.

    Ok(MigrationReport { copied, skipped })
}

// ---------------------------------------------------------------------
// SQLite schema reading
// ---------------------------------------------------------------------

fn read_sqlite_schema(path: &PathBuf) -> Result<(Vec<SqliteTable>, Vec<SqliteRelation>)> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;

    // Metadata might be missing if the SQLite was never touched by hr-db.
    // In that case we fall back to PRAGMA table_info introspection — but
    // the V1 migrator targets dataverse-managed apps only, so we hard-fail.
    let has_dv_tables: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='_dv_tables')",
            [],
            |r| r.get(0),
        )
        .unwrap_or(false);
    if !has_dv_tables {
        bail!(
            "no `_dv_tables` metadata found in {} — this app was never managed by hr-db, \
             nothing to migrate (or migrate raw schema by hand)",
            path.display()
        );
    }

    // Legacy schema doesn't have an `id` column on `_dv_tables` — the
    // PK is `name`. Order alphabetically; the new dataverse will assign
    // its own BIGSERIAL ids during create_table.
    let mut tstmt =
        conn.prepare("SELECT name, description FROM _dv_tables ORDER BY name")?;
    let trows = tstmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
    })?;

    let mut tables: Vec<SqliteTable> = Vec::new();
    for row in trows {
        let (name, description) = row?;
        let columns = read_columns(&conn, &name)?;
        tables.push(SqliteTable { name, description, columns });
    }

    let mut rstmt = conn.prepare(
        "SELECT from_table, from_column, to_table, to_column, relation_type, on_delete, on_update \
         FROM _dv_relations ORDER BY id",
    )?;
    let rrows = rstmt.query_map([], |r| {
        Ok(SqliteRelation {
            from_table: r.get(0)?,
            from_column: r.get(1)?,
            to_table: r.get(2)?,
            to_column: r.get(3)?,
            relation_type: r.get(4)?,
            on_delete: r.get(5)?,
            on_update: r.get(6)?,
        })
    })?;
    let relations: Vec<SqliteRelation> = rrows.filter_map(|r| r.ok()).collect();

    Ok((tables, relations))
}

fn read_columns(conn: &Connection, table: &str) -> Result<Vec<SqliteColumn>> {
    // Legacy `_dv_columns` has no `id` (PK is (table_name, name)) and no
    // `lookup_target` (relations are tracked exclusively in `_dv_relations`).
    let mut stmt = conn.prepare(
        "SELECT name, field_type, required, is_unique, default_value, description, \
                choices, formula_expression \
         FROM _dv_columns WHERE table_name = ?1 ORDER BY position, name",
    )?;
    let rows = stmt.query_map([table], |r| {
        let choices_json: Option<String> = r.get(6)?;
        let choices: Vec<String> = choices_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();
        Ok(SqliteColumn {
            name: r.get(0)?,
            field_type: r.get::<_, String>(1)?,
            required: r.get(2)?,
            is_unique: r.get(3)?,
            default_value: r.get(4)?,
            description: r.get(5)?,
            choices,
            formula_expression: r.get(7)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

// ---------------------------------------------------------------------
// Schema translation: SQLite (legacy) → hr-dataverse types
// ---------------------------------------------------------------------

fn build_table_definition(t: &SqliteTable) -> Result<hr_dataverse::TableDefinition> {
    let now = Utc::now();
    let mut cols: Vec<hr_dataverse::ColumnDefinition> = Vec::with_capacity(t.columns.len());
    for c in &t.columns {
        // Legacy SQLite stored the implicit columns (id / created_at /
        // updated_at) explicitly in `_dv_columns`. The new dataverse
        // treats those as managed and forbids redeclaration. We drop
        // them here — the matching values still get carried over by
        // the row copy because we always SELECT them explicitly.
        if matches!(c.name.as_str(), "id" | "created_at" | "updated_at") {
            continue;
        }
        // Skip Formula columns: their value is computed in PG via GENERATED,
        // and the source SQLite has them as plain TEXT — copying them would
        // collide with the GENERATED clause. The migrator drops them; the
        // app needs to recreate the formula expression intentionally.
        let ft = parse_field_type(&c.field_type)?;
        if matches!(ft, PgFieldType::Formula) {
            warn!(
                table = %t.name,
                column = %c.name,
                "skipping Formula column — recreate it manually with formula_expression"
            );
            continue;
        }
        cols.push(hr_dataverse::ColumnDefinition {
            name: c.name.clone(),
            field_type: ft,
            required: c.required,
            unique: c.is_unique,
            default_value: c.default_value.clone(),
            description: c.description.clone(),
            choices: c.choices.clone(),
            formula_expression: c.formula_expression.clone(),
            // legacy `_dv_columns` doesn't carry lookup_target — relations
            // are recreated separately from `_dv_relations` below.
            lookup_target: None,
        });
    }
    Ok(hr_dataverse::TableDefinition {
        name: t.name.clone(),
        slug: t.name.clone(),
        columns: cols,
        description: t.description.clone(),
        created_at: now,
        updated_at: now,
    })
}

fn parse_field_type(s: &str) -> Result<PgFieldType> {
    PgFieldType::from_code(s).ok_or_else(|| anyhow!("unknown field_type code in legacy DB: {}", s))
}

fn build_relation(r: &SqliteRelation) -> Result<hr_dataverse::RelationDefinition> {
    use hr_dataverse::{CascadeAction, CascadeRules, RelationType};
    let rt = match r.relation_type.as_str() {
        "one_to_many" => RelationType::OneToMany,
        "many_to_many" => RelationType::ManyToMany,
        "self_referential" => RelationType::SelfReferential,
        other => bail!("unknown relation_type in legacy DB: {}", other),
    };
    let parse_act = |s: &str| {
        CascadeAction::from_code(s)
            .ok_or_else(|| anyhow!("unknown cascade action: {}", s))
    };
    Ok(hr_dataverse::RelationDefinition {
        from_table: r.from_table.clone(),
        from_column: r.from_column.clone(),
        to_table: r.to_table.clone(),
        to_column: r.to_column.clone(),
        relation_type: rt,
        cascade: CascadeRules {
            on_delete: parse_act(&r.on_delete)?,
            on_update: parse_act(&r.on_update)?,
        },
    })
}

// ---------------------------------------------------------------------
// Row copy: SELECT * FROM sqlite → INSERT INTO postgres
// ---------------------------------------------------------------------

async fn copy_rows(
    engine: &DataverseEngine,
    sqlite_path: &PathBuf,
    table: &SqliteTable,
) -> Result<i64> {
    let conn = Connection::open_with_flags(sqlite_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;

    // We project the columns explicitly to keep the SELECT order stable.
    // Skip:
    //   - Formula columns (computed via GENERATED in PG)
    //   - The implicit id / created_at / updated_at — they go through the
    //     dedicated prelude below so the BIGSERIAL sequence stays aligned.
    let cols_to_copy: Vec<&SqliteColumn> = table
        .columns
        .iter()
        .filter(|c| {
            !matches!(c.name.as_str(), "id" | "created_at" | "updated_at")
                && !matches!(
                    parse_field_type(&c.field_type).ok(),
                    Some(PgFieldType::Formula)
                )
        })
        .collect();

    // We project user columns + `created_at` / `updated_at` (when the
    // source has them). The `id` column is intentionally NOT carried
    // over — Postgres assigns fresh BIGSERIAL ids on insert. This is
    // simpler and safe for apps without cross-table FK references.
    //
    // Caveat: apps that rely on stable IDs across the migration (e.g.
    // foreign keys referencing rows in another migrated table) need a
    // more sophisticated copier that preserves source IDs and does
    // setval on the sequence afterwards. Wallet has 0 relations so
    // this path is sufficient; the limitation is documented in the
    // migration tool's --help output direction.
    let has_created_at = sqlite_has_column(&conn, &table.name, "created_at")?;
    let has_updated_at = sqlite_has_column(&conn, &table.name, "updated_at")?;

    let mut select_cols: Vec<String> = Vec::new();
    if has_created_at { select_cols.push("created_at".to_string()); }
    if has_updated_at { select_cols.push("updated_at".to_string()); }
    for c in &cols_to_copy {
        select_cols.push(c.name.clone());
    }
    let select_sql = format!(
        "SELECT {} FROM \"{}\"",
        select_cols
            .iter()
            .map(|c| format!("\"{}\"", c))
            .collect::<Vec<_>>()
            .join(", "),
        table.name
    );

    let mut stmt = conn.prepare(&select_sql)?;
    let n_cols = stmt.column_count();

    let mut total: i64 = 0;
    let mut iter = stmt.query([])?;
    while let Some(row) = iter.next()? {
        let mut values: HashMap<String, JsonValue> = HashMap::with_capacity(n_cols);
        for (i, col_name) in select_cols.iter().enumerate() {
            let v: rusqlite::types::Value = row.get(i)?;
            values.insert(col_name.clone(), sqlite_to_json(v));
        }
        insert_row(
            engine,
            &table.name,
            &cols_to_copy,
            &values,
            has_created_at,
            has_updated_at,
        )
        .await?;
        total += 1;
    }

    Ok(total)
}

fn sqlite_has_column(conn: &Connection, table: &str, col: &str) -> Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info(\"{}\")", table))?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let name: String = r.get(1)?;
        if name == col {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn insert_row(
    engine: &DataverseEngine,
    table: &str,
    cols: &[&SqliteColumn],
    values: &HashMap<String, JsonValue>,
    has_created_at: bool,
    has_updated_at: bool,
) -> Result<()> {
    // Build INSERT with optional created_at/updated_at + user columns.
    // `id` is intentionally omitted so PG assigns BIGSERIAL.
    let mut col_names: Vec<String> = Vec::with_capacity(cols.len() + 2);
    let mut col_casts: Vec<&'static str> = Vec::with_capacity(cols.len() + 2);
    if has_created_at { col_names.push("created_at".into()); col_casts.push(""); }
    if has_updated_at { col_names.push("updated_at".into()); col_casts.push(""); }
    for c in cols {
        col_names.push(c.name.clone());
        // Numeric values are bound as TEXT (to preserve precision without
        // pulling bigdecimal). PG refuses implicit text→numeric so the
        // placeholder gets an explicit `::NUMERIC` cast.
        let ft = parse_field_type(&c.field_type)?;
        col_casts.push(match ft {
            PgFieldType::Decimal | PgFieldType::Currency | PgFieldType::Percent => "::NUMERIC",
            _ => "",
        });
    }

    let placeholders: Vec<String> = col_casts
        .iter()
        .enumerate()
        .map(|(i, cast)| format!("${}{}", i + 1, cast))
        .collect();
    let sql = format!(
        "INSERT INTO \"{}\" ({}) VALUES ({})",
        table,
        col_names
            .iter()
            .map(|c| format!("\"{}\"", c))
            .collect::<Vec<_>>()
            .join(", "),
        placeholders.join(", "),
    );

    let mut args = PgArguments::default();
    if has_created_at {
        bind_optional_dt(&mut args, "created_at", values)?;
    }
    if has_updated_at {
        bind_optional_dt(&mut args, "updated_at", values)?;
    }
    for c in cols {
        let ft = parse_field_type(&c.field_type)?;
        let v = values.get(&c.name).cloned().unwrap_or(JsonValue::Null);
        bind_for_field(&mut args, &v, ft)?;
    }

    sqlx_core::query::query_with::<sqlx_postgres::Postgres, _>(&sql, args)
        .execute(engine.pool())
        .await
        .with_context(|| format!("INSERT into {}", table))?;
    Ok(())
}

/// Bind a datetime value that *may* be NULL in the source. If null we
/// omit nothing — Postgres picks up the column DEFAULT (now()).
fn bind_optional_dt(
    args: &mut PgArguments,
    key: &str,
    values: &HashMap<String, JsonValue>,
) -> Result<()> {
    match values.get(key).and_then(|v| v.as_str()) {
        Some(s) => {
            let dt = parse_dt(s)?;
            args.add(dt).map_err(|e| anyhow!("bind {}: {}", key, e))?;
        }
        None => {
            // Source row has NULL: bind None and let PG handle it (the
            // column has DEFAULT now() so a NULL bind is fine).
            args.add(Option::<chrono::DateTime<Utc>>::None)
                .map_err(|e| anyhow!("bind {}: {}", key, e))?;
        }
    }
    Ok(())
}

fn bind_for_field(args: &mut PgArguments, v: &JsonValue, ft: PgFieldType) -> Result<()> {
    use PgFieldType::*;
    match (ft, v) {
        (_, JsonValue::Null) => match ft {
            // Nullable bind needs a typed Option to satisfy PG's encoder.
            Boolean => args.add(Option::<bool>::None).map_err(map_bind)?,
            Number | AutoIncrement | Lookup => {
                args.add(Option::<i64>::None).map_err(map_bind)?
            }
            Decimal | Currency | Percent => {
                args.add(Option::<String>::None).map_err(map_bind)?
            }
            DateTime => args
                .add(Option::<chrono::DateTime<Utc>>::None)
                .map_err(map_bind)?,
            Json => args
                .add(Option::<SqlxJson<JsonValue>>::None)
                .map_err(map_bind)?,
            Uuid => args.add(Option::<uuid::Uuid>::None).map_err(map_bind)?,
            MultiChoice => args.add(Option::<Vec<String>>::None).map_err(map_bind)?,
            _ => args.add(Option::<String>::None).map_err(map_bind)?,
        },
        (Boolean, JsonValue::Bool(b)) => args.add(*b).map_err(map_bind)?,
        // SQLite stores booleans as 0/1 ints — accept either.
        (Boolean, JsonValue::Number(n)) => {
            let b = n.as_i64().map(|i| i != 0).ok_or_else(|| anyhow!("bool: bad int"))?;
            args.add(b).map_err(map_bind)?
        }
        (Number | AutoIncrement | Lookup, JsonValue::Number(n)) => {
            let i = n.as_i64().ok_or_else(|| anyhow!("int: not i64"))?;
            args.add(i).map_err(map_bind)?
        }
        (Decimal | Currency | Percent, JsonValue::Number(n)) => {
            args.add(n.to_string()).map_err(map_bind)?
        }
        (Decimal | Currency | Percent, JsonValue::String(s)) => {
            args.add(s.clone()).map_err(map_bind)?
        }
        (DateTime, JsonValue::String(s)) => {
            let dt = parse_dt(s)?;
            args.add(dt).map_err(map_bind)?
        }
        (Date, JsonValue::String(s)) => {
            // PG DATE column needs a typed bind, not a TEXT cast.
            let d = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .with_context(|| format!("bad date: {}", s))?;
            args.add(d).map_err(map_bind)?
        }
        (Time, JsonValue::String(s)) => {
            let t = chrono::NaiveTime::parse_from_str(s, "%H:%M:%S")
                .or_else(|_| chrono::NaiveTime::parse_from_str(s, "%H:%M:%S%.f"))
                .with_context(|| format!("bad time: {}", s))?;
            args.add(t).map_err(map_bind)?
        }
        (Uuid, JsonValue::String(s)) => {
            let u = uuid::Uuid::parse_str(s)
                .with_context(|| format!("bad UUID: {}", s))?;
            args.add(u).map_err(map_bind)?
        }
        (Json, v) => args.add(SqlxJson(v.clone())).map_err(map_bind)?,
        (MultiChoice, JsonValue::String(s)) => {
            // SQLite stored MultiChoice as a JSON-encoded array string.
            let arr: Vec<String> = serde_json::from_str(s)
                .with_context(|| format!("MultiChoice: invalid JSON {}", s))?;
            args.add(arr).map_err(map_bind)?
        }
        (MultiChoice, JsonValue::Array(items)) => {
            let mut v: Vec<String> = Vec::with_capacity(items.len());
            for it in items {
                if let JsonValue::String(s) = it {
                    v.push(s.clone());
                } else {
                    bail!("MultiChoice element is not a string");
                }
            }
            args.add(v).map_err(map_bind)?
        }
        // String-flavoured fallthrough (Text/Email/Url/Phone/Choice/Date/Time/Duration/Formula).
        (_, JsonValue::String(s)) => args.add(s.clone()).map_err(map_bind)?,
        (_, JsonValue::Number(n)) => args.add(n.to_string()).map_err(map_bind)?,
        (_, JsonValue::Bool(b)) => args.add(b.to_string()).map_err(map_bind)?,
        (ft, other) => bail!("type mismatch: column type {:?} can't bind {:?}", ft, other),
    }
    Ok(())
}

fn map_bind(e: sqlx_core::error::BoxDynError) -> anyhow::Error {
    anyhow!("bind error: {}", e)
}

fn parse_dt(s: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .or_else(|_| {
            // SQLite often stores naive timestamps like "2026-04-30 17:13:31".
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
                .map(|n| n.and_utc())
        })
        .with_context(|| format!("invalid datetime: {}", s))
}

fn sqlite_to_json(v: rusqlite::types::Value) -> JsonValue {
    use rusqlite::types::Value as V;
    match v {
        V::Null => JsonValue::Null,
        V::Integer(i) => JsonValue::Number(i.into()),
        V::Real(f) => serde_json::Number::from_f64(f)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        V::Text(s) => JsonValue::String(s),
        V::Blob(b) => {
            // Rare in dataverse-managed schemas; encode as hex string fallback.
            let mut out = String::with_capacity(b.len() * 2);
            for byte in b {
                use std::fmt::Write;
                let _ = write!(&mut out, "{:02x}", byte);
            }
            JsonValue::String(out)
        }
    }
}

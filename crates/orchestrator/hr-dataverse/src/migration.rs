//! DDL generation for Postgres.
//!
//! Each user table gets three implicit columns:
//! - `id BIGSERIAL PRIMARY KEY`
//! - `created_at TIMESTAMPTZ NOT NULL DEFAULT now()`
//! - `updated_at TIMESTAMPTZ NOT NULL DEFAULT now()` (kept fresh by a trigger)
//!
//! The shared trigger function `_dv_set_updated_at()` is created once at
//! provisioning time (see [`crate::engine::INIT_METADATA_SQL`]), and a
//! per-table `BEFORE UPDATE` trigger fires it.

use crate::schema::{
    ColumnDefinition, DatabaseSchema, FieldType, IdStrategy, RelationDefinition, TableDefinition,
};

/// Quote a Postgres identifier with double quotes, escaping internal quotes.
///
/// We forbid most pathological cases via [`crate::validation`], but we still
/// quote everything for safety against case-folding surprises.
pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Build the column-fragment of a CREATE TABLE / ALTER TABLE ADD COLUMN.
///
/// `lookup_target_strategy` is the [`IdStrategy`] of the table this column
/// references, when [`ColumnDefinition::field_type`] is [`FieldType::Lookup`].
/// It dictates whether the FK column is BIGINT (target=Bigserial) or UUID
/// (target=Uuid). For non-Lookup columns this argument is ignored.
fn column_fragment(col: &ColumnDefinition, lookup_target_strategy: IdStrategy) -> String {
    let pg_type = if col.field_type == FieldType::Lookup {
        lookup_target_strategy.pg_fk_type()
    } else {
        col.field_type.pg_type()
    };
    let mut frag = format!("{} {}", quote_ident(&col.name), pg_type);

    if col.field_type == FieldType::Formula {
        // GENERATED ALWAYS AS (...) STORED. The user-provided expression is
        // inserted verbatim — we trust the schema-ops caller to pre-validate.
        if let Some(expr) = &col.formula_expression {
            frag.push_str(&format!(" GENERATED ALWAYS AS ({}) STORED", expr));
        }
    } else {
        if col.required {
            frag.push_str(" NOT NULL");
        }
        if col.unique {
            frag.push_str(" UNIQUE");
        }
        if let Some(default) = &col.default_value {
            frag.push_str(&format!(" DEFAULT {}", default));
        }
    }

    // NOTE on Choice columns: we deliberately do NOT emit a `CHECK
    // (col IN (…))` constraint anymore. Two reasons:
    //
    // 1. Migration safety — historical rows often carry values that
    //    fall outside the current choice set (`_dv_columns.choices` is
    //    a snapshot of the schema's *intended* values; SQLite never
    //    enforced anything). A CHECK would refuse legitimate data
    //    being copied from the legacy DB.
    //
    // 2. Choice values evolve — adding a value to a Choice column would
    //    require an `ALTER TABLE … DROP CONSTRAINT … ADD CONSTRAINT …`
    //    dance that complicates `add_column` / `add_choice` flows.
    //
    // The `choices` metadata stays in `_dv_columns` for documentation,
    // UI dropdowns, and GraphQL enum hinting. Enforcement of values
    // happens at the application/GraphQL layer, not in the storage
    // engine. If a strict CHECK is wanted post-migration, the operator
    // adds it manually.

    frag
}

/// Generate `CREATE TABLE` for a user table (without FK constraints — those
/// are applied via [`add_foreign_key_sql`] after every Lookup column exists).
///
/// `schema` is the existing schema (used to resolve Lookup columns' target
/// `id_strategy` so the FK column type matches the target's `id`).
pub fn create_table_sql(def: &TableDefinition, schema: &DatabaseSchema) -> String {
    let id_line = format!(
        "\"id\" {}{} PRIMARY KEY",
        def.id_strategy.pg_id_type(),
        def.id_strategy.id_default_clause(),
    );
    let mut cols: Vec<String> = vec![
        id_line,
        "\"created_at\" TIMESTAMPTZ NOT NULL DEFAULT now()".into(),
        "\"updated_at\" TIMESTAMPTZ NOT NULL DEFAULT now()".into(),
    ];
    for col in &def.columns {
        cols.push(column_fragment(col, lookup_strategy_for(col, def, schema)));
    }
    format!(
        "CREATE TABLE {} (\n  {}\n);",
        quote_ident(&def.name),
        cols.join(",\n  ")
    )
}

/// Resolve the [`IdStrategy`] of a Lookup column's target table. Returns
/// the default ([`IdStrategy::Bigserial`]) when the column isn't a Lookup
/// or when the target can't be resolved (caller validates target existence).
///
/// Allows self-FKs (target == def.name) — for a brand-new table being
/// created, its strategy isn't in `schema` yet, so we read it from `def`.
pub(crate) fn lookup_strategy_for(
    col: &ColumnDefinition,
    def: &TableDefinition,
    schema: &DatabaseSchema,
) -> IdStrategy {
    if col.field_type != FieldType::Lookup {
        return IdStrategy::default();
    }
    let Some(target) = col.lookup_target.as_deref() else {
        return IdStrategy::default();
    };
    if target == def.name {
        return def.id_strategy;
    }
    schema
        .tables
        .iter()
        .find(|t| t.name == target)
        .map(|t| t.id_strategy)
        .unwrap_or_default()
}

/// Generate the `BEFORE UPDATE` trigger that keeps `updated_at` fresh.
pub fn create_updated_at_trigger_sql(table: &str) -> String {
    let trig = format!("_dv_trg_{}_updated_at", table);
    format!(
        "CREATE TRIGGER {trig} BEFORE UPDATE ON {tbl} \
         FOR EACH ROW EXECUTE FUNCTION _dv_set_updated_at();",
        trig = quote_ident(&trig),
        tbl = quote_ident(table),
    )
}

pub fn drop_table_sql(table: &str) -> String {
    format!("DROP TABLE IF EXISTS {} CASCADE;", quote_ident(table))
}

/// Generate `ALTER TABLE … ADD COLUMN`. `lookup_target_strategy` is consulted
/// only when the column is a Lookup (to pick the FK column type matching the
/// target's `id`). For non-Lookup columns its value is irrelevant.
pub fn add_column_sql(
    table: &str,
    col: &ColumnDefinition,
    lookup_target_strategy: IdStrategy,
) -> String {
    format!(
        "ALTER TABLE {} ADD COLUMN {};",
        quote_ident(table),
        column_fragment(col, lookup_target_strategy)
    )
}

pub fn drop_column_sql(table: &str, column: &str) -> String {
    format!(
        "ALTER TABLE {} DROP COLUMN IF EXISTS {} CASCADE;",
        quote_ident(table),
        quote_ident(column)
    )
}

/// Build the `ADD CONSTRAINT … FOREIGN KEY` for a Lookup relation.
pub fn add_foreign_key_sql(rel: &RelationDefinition) -> String {
    let cname = format!("fk_{}_{}", rel.from_table, rel.from_column);
    format!(
        "ALTER TABLE {tbl} ADD CONSTRAINT {cname} \
         FOREIGN KEY ({fc}) REFERENCES {ttbl}({tc}) \
         ON DELETE {od} ON UPDATE {ou};",
        tbl = quote_ident(&rel.from_table),
        cname = quote_ident(&cname),
        fc = quote_ident(&rel.from_column),
        ttbl = quote_ident(&rel.to_table),
        tc = quote_ident(&rel.to_column),
        od = rel.cascade.on_delete.as_sql(),
        ou = rel.cascade.on_update.as_sql(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::*;
    use chrono::Utc;

    fn def_table(name: &str, cols: Vec<ColumnDefinition>) -> TableDefinition {
        TableDefinition {
            name: name.to_string(),
            slug: name.to_string(),
            columns: cols,
            description: None,
            id_strategy: IdStrategy::Bigserial,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn empty_schema() -> DatabaseSchema {
        DatabaseSchema::default()
    }

    fn col(name: &str, ty: FieldType) -> ColumnDefinition {
        ColumnDefinition {
            name: name.to_string(),
            field_type: ty,
            required: false,
            unique: false,
            default_value: None,
            description: None,
            choices: vec![],
            formula_expression: None,
            lookup_target: None,
        }
    }

    #[test]
    fn create_table_includes_implicit_columns() {
        let sql = create_table_sql(
            &def_table("contacts", vec![col("email", FieldType::Email)]),
            &empty_schema(),
        );
        assert!(sql.contains("\"id\" BIGSERIAL PRIMARY KEY"));
        assert!(sql.contains("\"created_at\" TIMESTAMPTZ"));
        assert!(sql.contains("\"updated_at\" TIMESTAMPTZ"));
        assert!(sql.contains("\"email\" TEXT"));
    }

    #[test]
    fn create_table_uuid_strategy_emits_uuid_pk() {
        let mut def = def_table("files", vec![col("name", FieldType::Text)]);
        def.id_strategy = IdStrategy::Uuid;
        let sql = create_table_sql(&def, &empty_schema());
        assert!(sql.contains("\"id\" UUID DEFAULT gen_random_uuid() PRIMARY KEY"));
    }

    #[test]
    fn lookup_column_inherits_target_strategy_uuid() {
        // Target table 'folders' uses UUID PKs; a Lookup pointing to it
        // must declare its FK column as UUID, not BIGINT.
        let folders = TableDefinition {
            name: "folders".into(),
            slug: "folders".into(),
            columns: vec![],
            description: None,
            id_strategy: IdStrategy::Uuid,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let schema = DatabaseSchema {
            tables: vec![folders],
            relations: vec![],
            version: 1,
            updated_at: None,
        };
        let mut fk_col = col("folder_id", FieldType::Lookup);
        fk_col.lookup_target = Some("folders".into());
        let mut def = def_table("files", vec![fk_col]);
        def.id_strategy = IdStrategy::Uuid;
        let sql = create_table_sql(&def, &schema);
        assert!(sql.contains("\"folder_id\" UUID"), "FK should be UUID, got: {}", sql);
        assert!(!sql.contains("\"folder_id\" BIGINT"));
    }

    #[test]
    fn lookup_column_default_target_is_bigint() {
        let companies = TableDefinition {
            name: "companies".into(),
            slug: "companies".into(),
            columns: vec![],
            description: None,
            id_strategy: IdStrategy::Bigserial,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let schema = DatabaseSchema {
            tables: vec![companies],
            relations: vec![],
            version: 1,
            updated_at: None,
        };
        let mut fk_col = col("company_id", FieldType::Lookup);
        fk_col.lookup_target = Some("companies".into());
        let def = def_table("contacts", vec![fk_col]);
        let sql = create_table_sql(&def, &schema);
        assert!(sql.contains("\"company_id\" BIGINT"));
    }

    #[test]
    fn choice_does_not_emit_check_constraint() {
        // Choice columns no longer emit CHECK constraints — see the
        // long-form note in `column_fragment`. The DDL is just a TEXT
        // column; enforcement happens at the application layer.
        let mut c = col("status", FieldType::Choice);
        c.choices = vec!["open".into(), "closed".into()];
        let sql = create_table_sql(&def_table("tickets", vec![c]), &empty_schema());
        assert!(!sql.contains("CHECK"));
        assert!(sql.contains("\"status\" TEXT"));
    }

    #[test]
    fn formula_emits_generated_clause() {
        let mut c = col("full_name", FieldType::Formula);
        c.formula_expression = Some("first_name || ' ' || last_name".into());
        let sql = create_table_sql(&def_table("people", vec![c]), &empty_schema());
        assert!(sql.contains("GENERATED ALWAYS AS (first_name || ' ' || last_name) STORED"));
    }

    #[test]
    fn fk_sql_uses_cascade_rules() {
        let rel = RelationDefinition {
            from_table: "contacts".into(),
            from_column: "company_id".into(),
            to_table: "companies".into(),
            to_column: "id".into(),
            relation_type: RelationType::OneToMany,
            cascade: CascadeRules { on_delete: CascadeAction::SetNull, on_update: CascadeAction::Cascade },
        };
        let sql = add_foreign_key_sql(&rel);
        assert!(sql.contains("FOREIGN KEY (\"company_id\")"));
        assert!(sql.contains("REFERENCES \"companies\"(\"id\")"));
        assert!(sql.contains("ON DELETE SET NULL"));
        assert!(sql.contains("ON UPDATE CASCADE"));
    }

    #[test]
    fn quote_ident_escapes_quotes() {
        assert_eq!(quote_ident("a\"b"), "\"a\"\"b\"");
    }

    #[test]
    fn multichoice_uses_array() {
        let sql = create_table_sql(
            &def_table("posts", vec![col("tags", FieldType::MultiChoice)]),
            &empty_schema(),
        );
        assert!(sql.contains("\"tags\" TEXT[]"));
    }
}

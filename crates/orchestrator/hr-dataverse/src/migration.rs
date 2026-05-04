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

use crate::schema::{ColumnDefinition, FieldType, RelationDefinition, TableDefinition};

/// Quote a Postgres identifier with double quotes, escaping internal quotes.
///
/// We forbid most pathological cases via [`crate::validation`], but we still
/// quote everything for safety against case-folding surprises.
pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Build the column-fragment of a CREATE TABLE / ALTER TABLE ADD COLUMN.
fn column_fragment(col: &ColumnDefinition) -> String {
    let mut frag = format!("{} {}", quote_ident(&col.name), col.field_type.pg_type());

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

    // CHECK constraint for Choice fields with a finite set of allowed values.
    if col.field_type == FieldType::Choice && !col.choices.is_empty() {
        let escaped: Vec<String> = col
            .choices
            .iter()
            .map(|c| format!("'{}'", c.replace('\'', "''")))
            .collect();
        frag.push_str(&format!(" CHECK ({} IN ({}))", quote_ident(&col.name), escaped.join(", ")));
    }

    frag
}

/// Generate `CREATE TABLE` for a user table (without FK constraints — those
/// are applied via [`add_foreign_key_sql`] after every Lookup column exists).
pub fn create_table_sql(def: &TableDefinition) -> String {
    let mut cols: Vec<String> = vec![
        "\"id\" BIGSERIAL PRIMARY KEY".into(),
        "\"created_at\" TIMESTAMPTZ NOT NULL DEFAULT now()".into(),
        "\"updated_at\" TIMESTAMPTZ NOT NULL DEFAULT now()".into(),
    ];
    for col in &def.columns {
        cols.push(column_fragment(col));
    }
    format!(
        "CREATE TABLE {} (\n  {}\n);",
        quote_ident(&def.name),
        cols.join(",\n  ")
    )
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

pub fn add_column_sql(table: &str, col: &ColumnDefinition) -> String {
    format!(
        "ALTER TABLE {} ADD COLUMN {};",
        quote_ident(table),
        column_fragment(col)
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
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
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
        let sql = create_table_sql(&def_table("contacts", vec![col("email", FieldType::Email)]));
        assert!(sql.contains("\"id\" BIGSERIAL PRIMARY KEY"));
        assert!(sql.contains("\"created_at\" TIMESTAMPTZ"));
        assert!(sql.contains("\"updated_at\" TIMESTAMPTZ"));
        assert!(sql.contains("\"email\" TEXT"));
    }

    #[test]
    fn choice_emits_check_constraint() {
        let mut c = col("status", FieldType::Choice);
        c.choices = vec!["open".into(), "closed".into()];
        let sql = create_table_sql(&def_table("tickets", vec![c]));
        assert!(sql.contains("CHECK (\"status\" IN ('open', 'closed'))"));
    }

    #[test]
    fn formula_emits_generated_clause() {
        let mut c = col("full_name", FieldType::Formula);
        c.formula_expression = Some("first_name || ' ' || last_name".into());
        let sql = create_table_sql(&def_table("people", vec![c]));
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
        let sql = create_table_sql(&def_table("posts", vec![col("tags", FieldType::MultiChoice)]));
        assert!(sql.contains("\"tags\" TEXT[]"));
    }
}

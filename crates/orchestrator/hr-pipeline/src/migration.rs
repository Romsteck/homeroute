use hr_db::migration::{self, MigrationOp};
use hr_db::schema::{DatabaseSchema, RelationDefinition};

/// Compare two database schemas and return the migration operations needed
/// to bring the target in sync with the source.
///
/// - New tables in source (not in target) -> CreateTable
/// - Tables in target but not in source -> DropTable
/// - New columns in existing tables -> AddColumn
/// - Removed columns -> RemoveColumn
/// - New relations -> CreateRelation
/// - Removed relations -> DropRelation
pub fn diff_schemas(source: &DatabaseSchema, target: &DatabaseSchema) -> Vec<MigrationOp> {
    let mut ops = Vec::new();

    let source_table_names: Vec<&str> = source.tables.iter().map(|t| t.name.as_str()).collect();
    let target_table_names: Vec<&str> = target.tables.iter().map(|t| t.name.as_str()).collect();

    // New tables in source
    for table in &source.tables {
        if !target_table_names.contains(&table.name.as_str()) {
            ops.push(MigrationOp::CreateTable(table.clone()));
        }
    }

    // Dropped tables (in target but not in source)
    for table in &target.tables {
        if !source_table_names.contains(&table.name.as_str()) {
            ops.push(MigrationOp::DropTable {
                table: table.name.clone(),
            });
        }
    }

    // Column changes in existing tables
    for source_table in &source.tables {
        if let Some(target_table) = target.tables.iter().find(|t| t.name == source_table.name) {
            let source_col_names: Vec<&str> =
                source_table.columns.iter().map(|c| c.name.as_str()).collect();
            let target_col_names: Vec<&str> =
                target_table.columns.iter().map(|c| c.name.as_str()).collect();

            // New columns
            for col in &source_table.columns {
                if !target_col_names.contains(&col.name.as_str()) {
                    ops.push(MigrationOp::AddColumn {
                        table: source_table.name.clone(),
                        column: col.clone(),
                    });
                }
            }

            // Removed columns
            for col in &target_table.columns {
                if !source_col_names.contains(&col.name.as_str()) {
                    ops.push(MigrationOp::RemoveColumn {
                        table: source_table.name.clone(),
                        column: col.name.clone(),
                    });
                }
            }
        }
    }

    // Relation changes
    for rel in &source.relations {
        if !has_relation(&target.relations, rel) {
            ops.push(MigrationOp::CreateRelation {
                relation: rel.clone(),
            });
        }
    }

    for rel in &target.relations {
        if !has_relation(&source.relations, rel) {
            ops.push(MigrationOp::DropRelation {
                from_table: rel.from_table.clone(),
                from_column: rel.from_column.clone(),
                to_table: rel.to_table.clone(),
                to_column: rel.to_column.clone(),
            });
        }
    }

    ops
}

/// Check if a relation exists in a list (by matching table/column pairs).
fn has_relation(relations: &[RelationDefinition], needle: &RelationDefinition) -> bool {
    relations.iter().any(|r| {
        r.from_table == needle.from_table
            && r.from_column == needle.from_column
            && r.to_table == needle.to_table
            && r.to_column == needle.to_column
    })
}

/// Generate SQL statements from migration operations.
pub fn generate_migration_sql(ops: &[MigrationOp]) -> Vec<String> {
    ops.iter().flat_map(|op| migration::generate_ddl(op)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use hr_db::schema::*;

    fn make_column(name: &str, ft: FieldType) -> ColumnDefinition {
        ColumnDefinition {
            name: name.to_string(),
            field_type: ft,
            required: false,
            unique: false,
            default_value: None,
            description: None,
            choices: vec![],
        }
    }

    fn make_table(name: &str, columns: Vec<ColumnDefinition>) -> TableDefinition {
        TableDefinition {
            name: name.to_string(),
            slug: name.to_lowercase(),
            columns,
            description: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_empty_schemas_no_ops() {
        let source = DatabaseSchema::default();
        let target = DatabaseSchema::default();
        let ops = diff_schemas(&source, &target);
        assert!(ops.is_empty());
    }

    #[test]
    fn test_identical_schemas_no_ops() {
        let table = make_table("users", vec![make_column("email", FieldType::Email)]);
        let source = DatabaseSchema {
            tables: vec![table.clone()],
            ..Default::default()
        };
        let target = DatabaseSchema {
            tables: vec![table],
            ..Default::default()
        };
        let ops = diff_schemas(&source, &target);
        assert!(ops.is_empty());
    }

    #[test]
    fn test_new_table_in_source() {
        let source = DatabaseSchema {
            tables: vec![make_table(
                "orders",
                vec![make_column("amount", FieldType::Decimal)],
            )],
            ..Default::default()
        };
        let target = DatabaseSchema::default();
        let ops = diff_schemas(&source, &target);
        assert_eq!(ops.len(), 1);
        assert!(matches!(&ops[0], MigrationOp::CreateTable(t) if t.name == "orders"));

        let sql = generate_migration_sql(&ops);
        assert_eq!(sql.len(), 1);
        assert!(sql[0].contains("CREATE TABLE"));
        assert!(sql[0].contains("orders"));
    }

    #[test]
    fn test_dropped_table() {
        let source = DatabaseSchema::default();
        let target = DatabaseSchema {
            tables: vec![make_table("legacy", vec![])],
            ..Default::default()
        };
        let ops = diff_schemas(&source, &target);
        assert_eq!(ops.len(), 1);
        assert!(matches!(&ops[0], MigrationOp::DropTable { table } if table == "legacy"));
    }

    #[test]
    fn test_new_column() {
        let source = DatabaseSchema {
            tables: vec![make_table(
                "users",
                vec![
                    make_column("email", FieldType::Email),
                    make_column("phone", FieldType::Phone),
                ],
            )],
            ..Default::default()
        };
        let target = DatabaseSchema {
            tables: vec![make_table(
                "users",
                vec![make_column("email", FieldType::Email)],
            )],
            ..Default::default()
        };
        let ops = diff_schemas(&source, &target);
        assert_eq!(ops.len(), 1);
        assert!(
            matches!(&ops[0], MigrationOp::AddColumn { table, column } if table == "users" && column.name == "phone")
        );

        let sql = generate_migration_sql(&ops);
        assert!(sql[0].contains("ALTER TABLE"));
        assert!(sql[0].contains("phone"));
    }

    #[test]
    fn test_removed_column() {
        let source = DatabaseSchema {
            tables: vec![make_table(
                "users",
                vec![make_column("email", FieldType::Email)],
            )],
            ..Default::default()
        };
        let target = DatabaseSchema {
            tables: vec![make_table(
                "users",
                vec![
                    make_column("email", FieldType::Email),
                    make_column("fax", FieldType::Text),
                ],
            )],
            ..Default::default()
        };
        let ops = diff_schemas(&source, &target);
        assert_eq!(ops.len(), 1);
        assert!(
            matches!(&ops[0], MigrationOp::RemoveColumn { table, column } if table == "users" && column == "fax")
        );
    }

    #[test]
    fn test_new_relation() {
        let rel = RelationDefinition {
            from_table: "orders".into(),
            from_column: "user_id".into(),
            to_table: "users".into(),
            to_column: "id".into(),
            relation_type: RelationType::OneToMany,
            cascade: CascadeRules::default(),
        };
        let source = DatabaseSchema {
            relations: vec![rel],
            ..Default::default()
        };
        let target = DatabaseSchema::default();
        let ops = diff_schemas(&source, &target);
        assert_eq!(ops.len(), 1);
        assert!(matches!(&ops[0], MigrationOp::CreateRelation { .. }));
    }

    #[test]
    fn test_complex_diff() {
        // Source: users (email, phone), orders (amount) with relation
        // Target: users (email, fax), legacy ()
        // Expected: AddColumn(users.phone), RemoveColumn(users.fax),
        //           CreateTable(orders), DropTable(legacy), CreateRelation
        let rel = RelationDefinition {
            from_table: "orders".into(),
            from_column: "user_id".into(),
            to_table: "users".into(),
            to_column: "id".into(),
            relation_type: RelationType::OneToMany,
            cascade: CascadeRules::default(),
        };
        let source = DatabaseSchema {
            tables: vec![
                make_table(
                    "users",
                    vec![
                        make_column("email", FieldType::Email),
                        make_column("phone", FieldType::Phone),
                    ],
                ),
                make_table("orders", vec![make_column("amount", FieldType::Decimal)]),
            ],
            relations: vec![rel],
            ..Default::default()
        };
        let target = DatabaseSchema {
            tables: vec![
                make_table(
                    "users",
                    vec![
                        make_column("email", FieldType::Email),
                        make_column("fax", FieldType::Text),
                    ],
                ),
                make_table("legacy", vec![]),
            ],
            ..Default::default()
        };

        let ops = diff_schemas(&source, &target);
        // CreateTable(orders), DropTable(legacy), AddColumn(phone), RemoveColumn(fax), CreateRelation
        assert_eq!(ops.len(), 5);

        let sql = generate_migration_sql(&ops);
        // Relations don't generate SQL, so 4 statements
        assert_eq!(sql.len(), 4);
    }
}

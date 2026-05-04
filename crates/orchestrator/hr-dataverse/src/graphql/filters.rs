//! Translate GraphQL filter inputs into parameterised SQL `WHERE` clauses.
//!
//! Operator vocabulary (Hasura-aligned, easy for LLMs to generate):
//!
//! - Per-field: `_eq`, `_ne`, `_gt`, `_gte`, `_lt`, `_lte`, `_in`, `_nin`,
//!   `_isNull`, plus `_like` / `_ilike` for `String` columns
//! - Boolean composition at the where root: `_and`, `_or`, `_not`
//!
//! Multiple operators on the same field are AND-ed:
//! `{age: {_gt: 5, _lt: 10}}` → `age > $1 AND age < $2`.

use async_graphql::Value;
use serde_json::Number;

use crate::error::{DataverseError, Result};
use crate::graphql::sql::{BindValue, SqlBuilder};
use crate::migration::quote_ident;
use crate::schema::{ColumnDefinition, FieldType, TableDefinition};

/// Build an SQL `WHERE` fragment from a GraphQL `where` argument value.
///
/// Returns `Ok(None)` when the filter is empty / null — the caller skips
/// the `WHERE` keyword in that case.
pub fn where_fragment(
    table: &TableDefinition,
    where_arg: Option<&Value>,
    builder: &mut SqlBuilder,
) -> Result<Option<String>> {
    let Some(v) = where_arg else { return Ok(None) };
    if matches!(v, Value::Null) {
        return Ok(None);
    }
    let Value::Object(map) = v else {
        return Err(DataverseError::internal(format!(
            "where: expected object, got {:?}", v
        )));
    };
    if map.is_empty() {
        return Ok(None);
    }
    let frag = compile_where(table, map, builder)?;
    Ok(Some(frag))
}

fn compile_where(
    table: &TableDefinition,
    map: &async_graphql::indexmap::IndexMap<async_graphql::Name, Value>,
    builder: &mut SqlBuilder,
) -> Result<String> {
    let mut parts: Vec<String> = Vec::new();

    for (key, val) in map.iter() {
        let key_str = key.as_str();
        match key_str {
            "_and" => {
                let Value::List(items) = val else {
                    return Err(DataverseError::internal("_and: expected list"));
                };
                if items.is_empty() { continue; }
                let mut sub: Vec<String> = Vec::with_capacity(items.len());
                for item in items {
                    if let Value::Object(m) = item {
                        sub.push(compile_where(table, m, builder)?);
                    }
                }
                if !sub.is_empty() {
                    parts.push(format!("({})", sub.join(" AND ")));
                }
            }
            "_or" => {
                let Value::List(items) = val else {
                    return Err(DataverseError::internal("_or: expected list"));
                };
                if items.is_empty() { continue; }
                let mut sub: Vec<String> = Vec::with_capacity(items.len());
                for item in items {
                    if let Value::Object(m) = item {
                        sub.push(compile_where(table, m, builder)?);
                    }
                }
                if !sub.is_empty() {
                    parts.push(format!("({})", sub.join(" OR ")));
                }
            }
            "_not" => {
                let Value::Object(m) = val else {
                    return Err(DataverseError::internal("_not: expected object"));
                };
                let inner = compile_where(table, m, builder)?;
                parts.push(format!("NOT ({})", inner));
            }
            field_name => {
                // Field condition: `field: { _op: value, ... }`
                let column = resolve_column(table, field_name)?;
                let Value::Object(ops) = val else {
                    return Err(DataverseError::internal(format!(
                        "field '{}': expected operator object, got {:?}", field_name, val
                    )));
                };
                let mut field_parts: Vec<String> = Vec::with_capacity(ops.len());
                for (op_name, op_val) in ops.iter() {
                    field_parts.push(compile_operator(
                        column,
                        op_name.as_str(),
                        op_val,
                        builder,
                    )?);
                }
                if !field_parts.is_empty() {
                    parts.push(field_parts.join(" AND "));
                }
            }
        }
    }

    if parts.is_empty() {
        Ok("TRUE".into())
    } else {
        Ok(parts.join(" AND "))
    }
}

fn compile_operator(
    column: &ColumnDefinition,
    op: &str,
    val: &Value,
    builder: &mut SqlBuilder,
) -> Result<String> {
    let col_sql = quote_ident(&column.name);

    match op {
        "_isNull" => {
            let Value::Boolean(b) = val else {
                return Err(DataverseError::internal("_isNull: expected boolean"));
            };
            Ok(if *b {
                format!("{} IS NULL", col_sql)
            } else {
                format!("{} IS NOT NULL", col_sql)
            })
        }
        "_eq" => binary_op(column, "=", val, builder),
        "_ne" => binary_op(column, "<>", val, builder),
        "_gt" => binary_op(column, ">", val, builder),
        "_gte" => binary_op(column, ">=", val, builder),
        "_lt" => binary_op(column, "<", val, builder),
        "_lte" => binary_op(column, "<=", val, builder),
        "_like" => string_op(column, "LIKE", val, builder),
        "_ilike" => string_op(column, "ILIKE", val, builder),
        "_in" => array_op(column, "= ANY", val, builder),
        "_nin" => array_op(column, "<> ALL", val, builder),
        other => Err(DataverseError::internal(format!(
            "unknown operator '{}' on column '{}'", other, column.name
        ))),
    }
}

fn binary_op(
    column: &ColumnDefinition,
    sql_op: &str,
    val: &Value,
    builder: &mut SqlBuilder,
) -> Result<String> {
    let Some(bind) = crate::graphql::sql::graphql_value_to_bind(val, column.field_type)? else {
        // `_eq: null` translates to IS NULL — convention used by Hasura.
        return Ok(format!(
            "{} {} NULL",
            quote_ident(&column.name),
            if sql_op == "=" { "IS" } else if sql_op == "<>" { "IS NOT" } else { sql_op }
        ));
    };
    let placeholder = builder.push(bind);
    Ok(format!("{} {} {}", quote_ident(&column.name), sql_op, placeholder))
}

fn string_op(
    column: &ColumnDefinition,
    sql_op: &str,
    val: &Value,
    builder: &mut SqlBuilder,
) -> Result<String> {
    let Value::String(s) = val else {
        return Err(DataverseError::internal(format!(
            "_like/_ilike requires String, got {:?}", val
        )));
    };
    let p = builder.push(BindValue::Text(s.clone()));
    Ok(format!("{} {} {}", quote_ident(&column.name), sql_op, p))
}

fn array_op(
    column: &ColumnDefinition,
    op_phrase: &str, // "= ANY" or "<> ALL"
    val: &Value,
    builder: &mut SqlBuilder,
) -> Result<String> {
    let Value::List(items) = val else {
        return Err(DataverseError::internal(format!(
            "_in/_nin requires List, got {:?}", val
        )));
    };
    if items.is_empty() {
        // Empty list: `_in: []` matches nothing, `_nin: []` matches everything.
        return Ok(if op_phrase == "= ANY" { "FALSE".into() } else { "TRUE".into() });
    }

    let bind = match column.field_type {
        FieldType::Number | FieldType::AutoIncrement | FieldType::Lookup => {
            let mut out: Vec<i64> = Vec::with_capacity(items.len());
            for it in items {
                let Value::Number(n) = it else {
                    return Err(DataverseError::internal("expected integers in _in/_nin"));
                };
                out.push(n.as_i64().ok_or_else(|| DataverseError::internal("non-integer in _in/_nin"))?);
            }
            BindValue::Json(serde_json::Value::Array(
                out.into_iter().map(|i| serde_json::Value::Number(Number::from(i))).collect(),
            ))
            // ^ We bind as JSONB and use jsonb_array_elements below.
            // This avoids the typed-array bind dance for now.
        }
        FieldType::Text | FieldType::Email | FieldType::Url | FieldType::Phone
        | FieldType::Choice => {
            let mut out: Vec<String> = Vec::with_capacity(items.len());
            for it in items {
                let Value::String(s) = it else {
                    return Err(DataverseError::internal("expected strings in _in/_nin"));
                };
                out.push(s.clone());
            }
            BindValue::TextArray(out)
        }
        ft => {
            return Err(DataverseError::internal(format!(
                "_in/_nin not supported on column type {:?}", ft
            )));
        }
    };

    // For the JSONB int path, we materialise the list as `(SELECT … jsonb_array_elements_text)`.
    // For the TextArray path, we use the plain `= ANY($N)`.
    let p = builder.push(bind);
    let col = quote_ident(&column.name);
    let sql = match column.field_type {
        FieldType::Number | FieldType::AutoIncrement | FieldType::Lookup => {
            format!(
                "{col} {op} (SELECT (value)::BIGINT FROM jsonb_array_elements({p}) AS arr(value))",
                col = col, op = op_phrase, p = p
            )
        }
        _ => format!("{} {} ({})", col, op_phrase, p),
    };
    Ok(sql)
}

fn resolve_column<'a>(table: &'a TableDefinition, name: &str) -> Result<&'a ColumnDefinition> {
    if name == "id" || name == "created_at" || name == "updated_at" {
        // Implicit columns: synthesise a stand-in ColumnDefinition for the
        // operator compiler. We never persist this — it's borrow-only.
        return Err(DataverseError::internal(format!(
            "filtering on implicit column '{}' is not yet supported", name
        )));
    }
    table
        .columns
        .iter()
        .find(|c| c.name == name)
        .ok_or_else(|| DataverseError::ColumnNotFound {
            table: table.name.clone(),
            column: name.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::*;
    use chrono::Utc;

    fn t() -> TableDefinition {
        TableDefinition {
            name: "contacts".into(),
            slug: "contacts".into(),
            columns: vec![
                ColumnDefinition {
                    name: "email".into(), field_type: FieldType::Email,
                    required: true, unique: false, default_value: None,
                    description: None, choices: vec![], formula_expression: None, lookup_target: None,
                },
                ColumnDefinition {
                    name: "age".into(), field_type: FieldType::Number,
                    required: false, unique: false, default_value: None,
                    description: None, choices: vec![], formula_expression: None, lookup_target: None,
                },
            ],
            description: None, created_at: Utc::now(), updated_at: Utc::now(),
        }
    }

    fn parse(json: &str) -> Value {
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        async_graphql::Value::from_json(v).unwrap()
    }

    #[test]
    fn simple_eq() {
        let arg = parse(r#"{"email":{"_eq":"a@b.c"}}"#);
        let mut b = SqlBuilder::new();
        let frag = where_fragment(&t(), Some(&arg), &mut b).unwrap().unwrap();
        assert_eq!(frag, "\"email\" = $1");
        assert_eq!(b.binds.len(), 1);
    }

    #[test]
    fn multi_op_same_field() {
        let arg = parse(r#"{"age":{"_gt":5,"_lt":10}}"#);
        let mut b = SqlBuilder::new();
        let frag = where_fragment(&t(), Some(&arg), &mut b).unwrap().unwrap();
        assert!(frag.contains("\"age\" > $1"));
        assert!(frag.contains("\"age\" < $2"));
    }

    #[test]
    fn and_combinator() {
        let arg = parse(r#"{"_and":[{"email":{"_eq":"a"}},{"age":{"_gt":5}}]}"#);
        let mut b = SqlBuilder::new();
        let frag = where_fragment(&t(), Some(&arg), &mut b).unwrap().unwrap();
        assert!(frag.contains("AND"));
        assert_eq!(b.binds.len(), 2);
    }

    #[test]
    fn isnull() {
        let arg = parse(r#"{"age":{"_isNull":true}}"#);
        let mut b = SqlBuilder::new();
        let frag = where_fragment(&t(), Some(&arg), &mut b).unwrap().unwrap();
        assert_eq!(frag, "\"age\" IS NULL");
    }

    #[test]
    fn ilike() {
        let arg = parse(r#"{"email":{"_ilike":"%@b.c"}}"#);
        let mut b = SqlBuilder::new();
        let frag = where_fragment(&t(), Some(&arg), &mut b).unwrap().unwrap();
        assert_eq!(frag, "\"email\" ILIKE $1");
    }

    #[test]
    fn empty_where() {
        let arg = parse("{}");
        let mut b = SqlBuilder::new();
        let frag = where_fragment(&t(), Some(&arg), &mut b).unwrap();
        assert!(frag.is_none());
    }

    #[test]
    fn unknown_column_errors() {
        let arg = parse(r#"{"phantom":{"_eq":"x"}}"#);
        let mut b = SqlBuilder::new();
        assert!(where_fragment(&t(), Some(&arg), &mut b).is_err());
    }
}

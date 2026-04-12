use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer};
use std::collections::HashMap;
use std::str::FromStr;

use super::types::{LogCategory, LogLevel, LogService};

/// Deserialize a comma-separated string into a Vec<T> for query params.
fn deserialize_comma_separated<'de, D, T>(deserializer: D) -> Result<Option<Vec<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    T::Err: std::fmt::Display,
{
    let opt: Option<String> = Option::deserialize(deserializer)?;
    match opt {
        None => Ok(None),
        Some(s) if s.is_empty() => Ok(None),
        Some(s) => {
            let items: Result<Vec<T>, _> = s.split(',').map(|v| v.trim().parse::<T>()).collect();
            match items {
                Ok(v) => Ok(Some(v)),
                Err(e) => Err(serde::de::Error::custom(format!("invalid value: {e}"))),
            }
        }
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct LogQuery {
    #[serde(default, deserialize_with = "deserialize_comma_separated")]
    pub level: Option<Vec<LogLevel>>,
    #[serde(default, deserialize_with = "deserialize_comma_separated")]
    pub service: Option<Vec<LogService>>,
    #[serde(default, deserialize_with = "deserialize_comma_separated")]
    pub category: Option<Vec<LogCategory>>,
    pub crate_name: Option<String>,
    pub module: Option<String>,
    pub source: Option<String>,
    pub q: Option<String>,
    pub request_id: Option<String>,
    pub user_id: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, serde::Serialize)]
pub struct LogStats {
    pub total: u64,
    pub by_level: HashMap<String, u64>,
    pub by_service: HashMap<String, u64>,
    pub db_size_bytes: u64,
    pub hot_count: usize,
    pub hot_capacity: usize,
}

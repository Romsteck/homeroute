use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Technology stack for an application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppStack {
    NextJs,
    AxumVite,
    Axum,
}

impl AppStack {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::NextJs => "Next.js",
            Self::AxumVite => "Vite+Rust",
            Self::Axum => "Rust Only",
        }
    }

    pub fn default_health_path(&self) -> &'static str {
        "/health"
    }
}

/// Whether an app is reachable without authentication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    Public,
    #[default]
    Private,
}

/// Runtime state of an app process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AppState {
    #[default]
    Stopped,
    Starting,
    Running,
    Crashed,
    Unknown,
}

pub fn valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= 64
        && slug.as_bytes()[0].is_ascii_lowercase()
        && slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !slug.ends_with('-')
}

/// An application managed by HomeRoute, running directly on the host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Application {
    pub slug: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub stack: AppStack,
    #[serde(default)]
    pub has_db: bool,
    #[serde(default)]
    pub visibility: Visibility,
    pub domain: String,
    pub port: u16,
    pub run_command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_command: Option<String>,
    /// Override the artefact path(s) to rsync back after a remote build.
    /// If None, defaults are derived from the stack (see `app.build` docs).
    /// Paths are relative to `src/`. Multiple paths separated by newline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_artefact: Option<String>,
    pub health_path: String,
    #[serde(default)]
    pub env_vars: BTreeMap<String, String>,
    #[serde(default)]
    pub state: AppState,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Application {
    /// Create a new application with sensible defaults for the given stack.
    pub fn new(slug: String, name: String, stack: AppStack) -> Self {
        let now = Utc::now();
        let domain = format!("{}.mynetwk.biz", slug);
        let health_path = stack.default_health_path().to_string();
        Self {
            slug,
            name,
            description: None,
            stack,
            has_db: false,
            visibility: Visibility::Private,
            domain,
            port: 0,
            run_command: String::new(),
            build_command: None,
            build_artefact: None,
            health_path,
            env_vars: BTreeMap::new(),
            state: AppState::Stopped,
            created_at: now,
            updated_at: now,
        }
    }

    /// Root directory for this app's source, build artifacts and DB.
    pub fn app_dir(&self) -> PathBuf {
        PathBuf::from(format!("/opt/homeroute/apps/{}", self.slug))
    }

    /// Path to the managed SQLite database for this app.
    pub fn db_path(&self) -> PathBuf {
        self.app_dir().join("db.sqlite")
    }

    /// Path to the runtime `.env` file.
    pub fn env_file(&self) -> PathBuf {
        self.app_dir().join(".env")
    }

    /// Path to the source tree for this app.
    pub fn src_dir(&self) -> PathBuf {
        self.app_dir().join("src")
    }
}

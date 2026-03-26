use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::sync::RwLock;
use tracing::{info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectStack {
    AxumViteReact,
    NextJs,
    AxumFlutter,
}

impl ProjectStack {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AxumViteReact => "axum-vite-react",
            Self::NextJs => "nextjs",
            Self::AxumFlutter => "axum-flutter",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectProd {
    pub container_name: String,
    pub ip: Option<String>,
    pub app_id: Option<String>,
    pub service: String,
    pub binary: Option<String>,
    pub static_dir: Option<String>,
    pub db_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub slug: String,
    pub name: String,
    pub stack: ProjectStack,
    pub dev_path: String,
    pub prod: ProjectProd,
    pub git_remote: Option<String>,
    pub domain: Option<String>,
    pub created_at: String,
    pub last_deployed_at: Option<String>,
    pub last_deploy_commit: Option<String>,
}

pub struct ProjectRegistry {
    path: PathBuf,
    projects: RwLock<Vec<Project>>,
}

impl ProjectRegistry {
    pub fn load_or_default(path: &str) -> Self {
        let path = PathBuf::from(path);
        let projects = match std::fs::read_to_string(&path) {
            Ok(content) => match serde_json::from_str::<Vec<Project>>(&content) {
                Ok(p) => {
                    info!(count = p.len(), path = %path.display(), "Loaded project registry");
                    p
                }
                Err(e) => {
                    warn!(error = %e, path = %path.display(), "Failed to parse project registry, starting empty");
                    Vec::new()
                }
            },
            Err(_) => {
                info!(path = %path.display(), "No project registry found, starting empty");
                Vec::new()
            }
        };
        Self {
            path,
            projects: RwLock::new(projects),
        }
    }

    fn save_sync(path: &PathBuf, projects: &[Project]) -> Result<(), String> {
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(projects)
            .map_err(|e| format!("Failed to serialize: {e}"))?;
        std::fs::write(&tmp, &json)
            .map_err(|e| format!("Failed to write {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path)
            .map_err(|e| format!("Failed to rename: {e}"))?;
        Ok(())
    }

    pub async fn get(&self, slug: &str) -> Option<Project> {
        self.projects.read().await.iter().find(|p| p.slug == slug).cloned()
    }

    pub async fn update(&self, slug: &str, f: impl FnOnce(&mut Project)) -> Result<(), String> {
        let mut projects = self.projects.write().await;
        let project = projects
            .iter_mut()
            .find(|p| p.slug == slug)
            .ok_or_else(|| format!("Project '{slug}' not found"))?;
        f(project);
        Self::save_sync(&self.path, &projects)
    }
}

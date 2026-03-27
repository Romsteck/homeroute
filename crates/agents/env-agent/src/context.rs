//! Claude Code context generation for per-app project awareness.
//!
//! Generates per-app context files so that Claude Code running in the studio
//! has full project awareness:
//!   - {apps_path}/{slug}/CLAUDE.md           — project identity, DB schema, commands
//!   - {apps_path}/{slug}/.claude/settings.json — MCP servers (env-agent, homeroute, hub)
//!   - {apps_path}/{slug}/.claude/rules/env-context.md  — permissions for this env type

use std::fs;
use std::path::{Path, PathBuf};

use hr_environment::config::EnvAgentAppConfig;
use hr_environment::types::{AppStackType, EnvPermissions, EnvType};
use tracing::{info, warn};

use crate::db_manager::DbManager;

/// Generates Claude Code context files for apps in an environment.
pub struct ContextGenerator {
    pub env_slug: String,
    pub env_type: EnvType,
    pub base_domain: String,
    pub apps_path: String,
    pub mcp_port: u16,
    pub homeroute_address: String,
    pub homeroute_port: u16,
}

impl ContextGenerator {
    /// Generate all context files for a single app.
    pub fn generate_for_app(
        &self,
        app: &EnvAgentAppConfig,
        db_tables: Option<Vec<String>>,
    ) -> anyhow::Result<()> {
        let app_dir = PathBuf::from(&self.apps_path).join(&app.slug);

        // Ensure directories exist
        let claude_dir = app_dir.join(".claude");
        let rules_dir = claude_dir.join("rules");
        fs::create_dir_all(&rules_dir)?;

        // 1. CLAUDE.md
        let claude_md = self.render_claude_md(app, &db_tables);
        write_if_changed(&app_dir.join("CLAUDE.md"), &claude_md)?;

        // 2. .claude/settings.json
        let settings = self.render_settings_json();
        write_if_changed(&claude_dir.join("settings.json"), &settings)?;

        // 3. .claude/rules/env-context.md
        let env_context = self.render_env_context_md();
        write_if_changed(&rules_dir.join("env-context.md"), &env_context)?;

        info!(
            app = %app.slug,
            env = %self.env_slug,
            "context files generated"
        );

        Ok(())
    }

    /// Refresh context files for all apps.
    pub fn refresh_all(&self, apps: &[EnvAgentAppConfig]) -> anyhow::Result<()> {
        for app in apps {
            if let Err(e) = self.generate_for_app(app, None) {
                warn!(app = %app.slug, error = %e, "failed to generate context");
            }
        }
        Ok(())
    }

    /// Generate context for an app by slug, fetching DB table names from the DbManager.
    /// Used by MCP tool `studio.refresh_context`.
    pub async fn generate_for_app_by_slug(
        &self,
        slug: &str,
        apps: &[EnvAgentAppConfig],
        db: &DbManager,
    ) -> anyhow::Result<()> {
        let app = apps
            .iter()
            .find(|a| a.slug == slug)
            .ok_or_else(|| anyhow::anyhow!("app not found: {}", slug))?;

        let db_tables = if app.has_db {
            match db.get_engine(slug).await {
                Ok(engine) => {
                    let engine = engine.lock().await;
                    engine
                        .get_schema()
                        .ok()
                        .map(|s| s.tables.iter().map(|t| t.name.clone()).collect())
                }
                Err(_) => None,
            }
        } else {
            None
        };

        self.generate_for_app(app, db_tables)
    }

    /// Refresh context for all apps, fetching DB info.
    /// Used by MCP tool `studio.refresh_all`.
    pub async fn generate_all_with_db(
        &self,
        apps: &[EnvAgentAppConfig],
        db: &DbManager,
    ) -> anyhow::Result<usize> {
        let mut count = 0;
        for app in apps {
            match self.generate_for_app_by_slug(&app.slug, apps, db).await {
                Ok(()) => count += 1,
                Err(e) => warn!(app = %app.slug, error = %e, "failed to generate context"),
            }
        }
        Ok(count)
    }

    // ── Renderers ──────────────────────────────────────────────────────

    fn render_claude_md(
        &self,
        app: &EnvAgentAppConfig,
        db_tables: &Option<Vec<String>>,
    ) -> String {
        let stack_label = stack_display(app.stack);
        let env_label = env_type_label(self.env_type);
        let url = format!("{}.{}.{}", app.slug, self.env_slug, self.base_domain);

        let db_section = match (app.has_db, db_tables) {
            (true, Some(tables)) if !tables.is_empty() => {
                let mut s = String::from("Tables disponibles :\n");
                for t in tables {
                    s.push_str(&format!("- `{}`\n", t));
                }
                s.push_str(
                    "\nUtiliser les outils MCP `db.*` pour interagir avec la base de donnees.",
                );
                s
            }
            (true, _) => {
                "Base de donnees activee (tables non encore detectees).\n\
                 Utiliser les outils MCP `db.*` pour interagir avec la base de donnees."
                    .to_string()
            }
            (false, _) => "Pas de base de donnees configuree.".to_string(),
        };

        let build_cmd = app
            .build_command
            .as_deref()
            .unwrap_or("N/A");

        format!(
            "# {name} — Environnement {env_label}\n\
             \n\
             ## Identite\n\
             - App: {name} (slug: {slug})\n\
             - Env: {env_slug} (studio.{env_slug}.mynetwk.biz)\n\
             - Stack: {stack}\n\
             - URL: {url}\n\
             \n\
             ## Base de donnees\n\
             {db_section}\n\
             \n\
             ## Commandes\n\
             - Build: {build_cmd}\n\
             - Run: {run_command}\n\
             - Health: curl http://localhost:{port}{health_path}\n\
             \n\
             ## Regles\n\
             Voir `.claude/rules/env-context.md` pour les permissions de cet environnement.\n",
            name = app.name,
            env_label = env_label,
            slug = app.slug,
            env_slug = self.env_slug,
            stack = stack_label,
            url = url,
            db_section = db_section,
            build_cmd = build_cmd,
            run_command = app.run_command,
            port = app.port,
            health_path = app.health_path,
        )
    }

    fn render_settings_json(&self) -> String {
        let settings = serde_json::json!({
            "mcpServers": {
                "env": {
                    "url": format!("http://localhost:{}/mcp", self.mcp_port)
                },
                "homeroute": {
                    "url": format!("http://{}:{}/mcp", self.homeroute_address, self.homeroute_port)
                },
                "hub": {
                    "url": "http://10.0.0.20:3500/mcp"
                }
            }
        });
        serde_json::to_string_pretty(&settings).expect("JSON serialization cannot fail")
    }

    fn render_env_context_md(&self) -> String {
        let perms = EnvPermissions::for_type(self.env_type);
        let env_label = env_type_label(self.env_type);

        let mut lines = Vec::new();
        lines.push(format!(
            "# Permissions — Environnement {}\n",
            env_label
        ));

        lines.push(format!(
            "| Permission | Autorise |"
        ));
        lines.push("| --- | --- |".to_string());
        lines.push(format!(
            "| Modifier le code source | {} |",
            yes_no(perms.code_edit)
        ));
        lines.push(format!(
            "| Build et run | {} |",
            yes_no(perms.build_run)
        ));
        lines.push(format!(
            "| Modifier le schema DB | {} |",
            yes_no(perms.db_schema_write)
        ));
        lines.push(format!(
            "| Ecrire des donnees DB | {} |",
            yes_no(perms.db_data_write)
        ));
        lines.push(format!(
            "| Lire des donnees DB | {} |",
            yes_no(perms.db_data_read)
        ));
        lines.push(format!(
            "| Lire les logs | {} |",
            yes_no(perms.logs_read)
        ));
        lines.push(format!(
            "| Promouvoir via pipeline | {} |",
            yes_no(perms.pipeline_promote)
        ));
        lines.push(format!(
            "| Rollback | {} |",
            yes_no(perms.pipeline_rollback)
        ));
        lines.push(format!(
            "| Modifier les variables d'env | {} |",
            yes_no(perms.env_vars_write)
        ));

        // Add explicit warnings for restricted envs
        match self.env_type {
            EnvType::Acceptance => {
                lines.push(String::new());
                lines.push(
                    "**Attention** : Environnement d'acceptation. Le code source est en lecture seule. \
                     Les ecritures DB sont limitees aux tests."
                        .to_string(),
                );
            }
            EnvType::Production => {
                lines.push(String::new());
                lines.push(
                    "**PRODUCTION** : Environnement verrouille. Code en lecture seule, \
                     pas d'ecriture DB directe, pas de build. Seuls rollback et lecture sont autorises."
                        .to_string(),
                );
            }
            EnvType::Development => {}
        }

        lines.push(String::new());
        lines.join("\n")
    }
}

// ── Helpers ────────────────────────────────────────────────────────────

fn stack_display(stack: AppStackType) -> &'static str {
    match stack {
        AppStackType::NextJs => "Next.js",
        AppStackType::AxumVite => "Axum + Vite/React",
        AppStackType::AxumFlutter => "Axum + Flutter",
    }
}

fn env_type_label(env_type: EnvType) -> &'static str {
    match env_type {
        EnvType::Development => "Development",
        EnvType::Acceptance => "Acceptance",
        EnvType::Production => "Production",
    }
}

fn yes_no(v: bool) -> &'static str {
    if v { "Oui" } else { "Non" }
}

/// Write a file only if the content has changed, to avoid unnecessary FS churn.
fn write_if_changed(path: &Path, content: &str) -> anyhow::Result<()> {
    if path.exists() {
        if let Ok(existing) = fs::read_to_string(path) {
            if existing == content {
                return Ok(());
            }
        }
    }
    fs::write(path, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_generator() -> ContextGenerator {
        ContextGenerator {
            env_slug: "dev".to_string(),
            env_type: EnvType::Development,
            base_domain: "mynetwk.biz".to_string(),
            apps_path: "/tmp/ctx-test-apps".to_string(),
            mcp_port: 4010,
            homeroute_address: "10.0.0.254".to_string(),
            homeroute_port: 4001,
        }
    }

    fn test_app() -> EnvAgentAppConfig {
        EnvAgentAppConfig {
            slug: "trader".to_string(),
            name: "Trader".to_string(),
            stack: AppStackType::AxumVite,
            port: 3001,
            run_command: "./bin/trader".to_string(),
            build_command: Some("cargo build --release".to_string()),
            health_path: "/api/health".to_string(),
            has_db: true,
        }
    }

    #[test]
    fn test_generate_creates_files() {
        let ctx = test_generator();
        let app = test_app();
        let base = PathBuf::from(&ctx.apps_path);

        // Clean up from previous runs
        let _ = fs::remove_dir_all(&base);

        let tables = vec!["users".to_string(), "trades".to_string()];
        ctx.generate_for_app(&app, Some(tables)).unwrap();

        let claude_md = fs::read_to_string(base.join("trader/CLAUDE.md")).unwrap();
        assert!(claude_md.contains("# Trader"));
        assert!(claude_md.contains("slug: trader"));
        assert!(claude_md.contains("Axum + Vite/React"));
        assert!(claude_md.contains("`users`"));
        assert!(claude_md.contains("`trades`"));
        assert!(claude_md.contains("cargo build --release"));

        let settings =
            fs::read_to_string(base.join("trader/.claude/settings.json")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&settings).unwrap();
        assert!(parsed["mcpServers"]["env"]["url"]
            .as_str()
            .unwrap()
            .contains("4010"));
        assert!(parsed["mcpServers"]["homeroute"]["url"]
            .as_str()
            .unwrap()
            .contains("10.0.0.254"));

        let env_ctx =
            fs::read_to_string(base.join("trader/.claude/rules/env-context.md")).unwrap();
        assert!(env_ctx.contains("Development"));
        assert!(env_ctx.contains("Oui"));

        // Clean up
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_no_db() {
        let ctx = test_generator();
        let mut app = test_app();
        app.has_db = false;

        let md = ctx.render_claude_md(&app, &None);
        assert!(md.contains("Pas de base de donnees configuree"));
    }

    #[test]
    fn test_prod_permissions() {
        let ctx = ContextGenerator {
            env_type: EnvType::Production,
            ..test_generator()
        };

        let ctx_md = ctx.render_env_context_md();
        assert!(ctx_md.contains("PRODUCTION"));
        assert!(ctx_md.contains("| Modifier le code source | Non |"));
        assert!(ctx_md.contains("| Rollback | Oui |"));
    }

    #[test]
    fn test_no_build_command() {
        let ctx = test_generator();
        let mut app = test_app();
        app.build_command = None;

        let md = ctx.render_claude_md(&app, &None);
        assert!(md.contains("Build: N/A"));
    }

    #[test]
    fn test_refresh_all() {
        let ctx = ContextGenerator {
            apps_path: "/tmp/ctx-test-refresh".to_string(),
            ..test_generator()
        };

        let _ = fs::remove_dir_all(&ctx.apps_path);

        let apps = vec![
            test_app(),
            EnvAgentAppConfig {
                slug: "wallet".to_string(),
                name: "Wallet".to_string(),
                stack: AppStackType::AxumVite,
                port: 3002,
                run_command: "./bin/wallet".to_string(),
                build_command: None,
                health_path: "/api/health".to_string(),
                has_db: false,
            },
        ];

        ctx.refresh_all(&apps).unwrap();

        assert!(PathBuf::from("/tmp/ctx-test-refresh/trader/CLAUDE.md").exists());
        assert!(PathBuf::from("/tmp/ctx-test-refresh/wallet/CLAUDE.md").exists());

        let _ = fs::remove_dir_all(&ctx.apps_path);
    }

    #[test]
    fn test_write_if_changed_no_rewrite() {
        let dir = PathBuf::from("/tmp/ctx-test-idempotent");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let file = dir.join("test.txt");
        write_if_changed(&file, "hello").unwrap();
        let mtime1 = fs::metadata(&file).unwrap().modified().unwrap();

        // Small delay so mtime would differ if rewritten
        std::thread::sleep(std::time::Duration::from_millis(50));

        write_if_changed(&file, "hello").unwrap();
        let mtime2 = fs::metadata(&file).unwrap().modified().unwrap();

        assert_eq!(mtime1, mtime2, "file should not be rewritten if unchanged");

        let _ = fs::remove_dir_all(&dir);
    }
}

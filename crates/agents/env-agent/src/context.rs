//! Claude Code context generation for per-app project awareness.
//!
//! Generates per-app context files so that Claude Code running in the studio
//! has full project awareness:
//!   - {apps_path}/{slug}/CLAUDE.md                  — project identity, DB schema
//!   - {apps_path}/{slug}/.claude/settings.json      — single MCP server (project-scoped)
//!   - {apps_path}/{slug}/.claude/rules/env-rules.md — permissions for this env type
//!   - {apps_path}/{slug}/.claude/rules/mcp-tools.md — MCP tools documentation
//!   - {apps_path}/{slug}/.claude/rules/workflow.md  — development workflow
//!
//! Also generates global context at the apps root:
//!   - {apps_path}/CLAUDE.md                      — environment overview, all apps
//!   - {apps_path}/.claude/settings.json          — single MCP server (no project scoping)
//!   - {apps_path}/.claude/rules/env-rules.md     — permissions for this env type

use std::fs;
use std::path::{Path, PathBuf};

use hr_environment::config::EnvAgentAppConfig;
use hr_environment::types::{EnvPermissions, EnvType};
use tracing::{info, warn};

use crate::db_manager::DbManager;

/// Generates Claude Code context files for apps in an environment.
pub struct ContextGenerator {
    pub env_slug: String,
    pub env_type: EnvType,
    pub base_domain: String,
    pub apps_path: String,
    pub mcp_port: u16,
}

impl ContextGenerator {
    /// Generate all context files for a single app.
    pub fn generate_for_app(
        &self,
        app: &EnvAgentAppConfig,
        all_apps: &[EnvAgentAppConfig],
        db_tables: Option<Vec<String>>,
    ) -> anyhow::Result<()> {
        let app_dir = PathBuf::from(&self.apps_path).join(&app.slug);
        let claude_dir = app_dir.join(".claude");
        let rules_dir = claude_dir.join("rules");
        fs::create_dir_all(&rules_dir)?;

        // 1. CLAUDE.md
        let claude_md = self.render_claude_md(app, all_apps, &db_tables);
        write_if_changed(&app_dir.join("CLAUDE.md"), &claude_md)?;

        // 2. .claude/settings.json (single MCP server, project-scoped)
        let settings = self.render_settings_json_for_app(&app.slug);
        write_if_changed(&claude_dir.join("settings.json"), &settings)?;

        // 3. .claude/rules/env-rules.md (permissions)
        let env_rules = self.render_env_rules_md();
        write_if_changed(&rules_dir.join("env-rules.md"), &env_rules)?;

        // 4. .claude/rules/mcp-tools.md (tool documentation)
        let mcp_tools = self.render_mcp_tools_md(app);
        write_if_changed(&rules_dir.join("mcp-tools.md"), &mcp_tools)?;

        // 5. .claude/rules/workflow.md (development workflow)
        let workflow = self.render_workflow_md(app);
        write_if_changed(&rules_dir.join("workflow.md"), &workflow)?;

        // 6. .mcp.json (Claude Code VS Code extension reads this for MCP servers)
        let mcp_json = self.render_mcp_json_for_app(&app.slug);
        write_if_changed(&app_dir.join(".mcp.json"), &mcp_json)?;

        info!(app = %app.slug, env = %self.env_slug, "context files generated");
        Ok(())
    }

    /// Refresh context files for all apps + global context.
    pub fn refresh_all(&self, apps: &[EnvAgentAppConfig]) -> anyhow::Result<()> {
        for app in apps {
            if let Err(e) = self.generate_for_app(app, apps, None) {
                warn!(app = %app.slug, error = %e, "failed to generate context");
            }
        }
        if let Err(e) = self.generate_global_context(apps) {
            warn!(error = %e, "failed to generate global context");
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

        self.generate_for_app(app, apps, db_tables)
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
        if let Err(e) = self.generate_global_context(apps) {
            warn!(error = %e, "failed to generate global context");
        }
        Ok(count)
    }

    /// Generate global context files at the apps root.
    pub fn generate_global_context(&self, apps: &[EnvAgentAppConfig]) -> anyhow::Result<()> {
        let root = PathBuf::from(&self.apps_path);
        let claude_dir = root.join(".claude");
        let rules_dir = claude_dir.join("rules");
        fs::create_dir_all(&rules_dir)?;

        // 1. /apps/CLAUDE.md
        let claude_md = self.render_global_claude_md(apps);
        write_if_changed(&root.join("CLAUDE.md"), &claude_md)?;

        // 2. /apps/.claude/settings.json (no project scoping at root level)
        let settings = self.render_settings_json_global();
        write_if_changed(&claude_dir.join("settings.json"), &settings)?;

        // 3. /apps/.claude/rules/env-rules.md
        let rules = self.render_env_rules_md();
        write_if_changed(&rules_dir.join("env-rules.md"), &rules)?;

        info!(env = %self.env_slug, "global context files generated");
        Ok(())
    }

    // ── Renderers ──────────────────────────────────────────────────────

    fn render_claude_md(
        &self,
        app: &EnvAgentAppConfig,
        all_apps: &[EnvAgentAppConfig],
        db_tables: &Option<Vec<String>>,
    ) -> String {
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

        let structure_section = match &app.description {
            Some(desc) => desc.clone(),
            None => app.stack.project_structure().to_string(),
        };

        let build_cmd = app
            .build_command
            .as_deref()
            .unwrap_or_else(|| app.stack.default_build_command());

        let service_section = if self.env_type == EnvType::Development {
            format!(
                "- Watch: `{slug}-watch.service` — rebuild continu\n  \
                 Chaque modification de code declenche un rebuild + restart automatique.\n  \
                 Pour rebuilder manuellement: `{build}` puis `systemctl restart {slug}`",
                slug = app.slug,
                build = build_cmd,
            )
        } else {
            "- Deploye via pipeline. Ne pas modifier le code directement.".to_string()
        };

        // Other apps section
        let other_apps: Vec<String> = all_apps
            .iter()
            .filter(|a| a.slug != app.slug)
            .map(|a| {
                format!(
                    "- {name} ({slug}) — port {port}, stack {stack}",
                    name = a.name,
                    slug = a.slug,
                    port = a.port,
                    stack = a.stack.display_name(),
                )
            })
            .collect();

        let other_apps_section = if other_apps.is_empty() {
            "Aucune autre app dans cet environnement.".to_string()
        } else {
            other_apps.join("\n")
        };

        format!(
            "# {name} — Environnement {env_label}\n\
             \n\
             ## Identite\n\
             - App: {name} (slug: {slug})\n\
             - Env: {env_slug} (studio.{env_slug}.mynetwk.biz)\n\
             - Stack: {stack}\n\
             - URL: {url}\n\
             - Port: {port}\n\
             \n\
             ## Structure du projet\n\
             {structure}\n\
             \n\
             ## Base de donnees\n\
             {db_section}\n\
             \n\
             ## Service systemd\n\
             - Service principal: `{slug}.service`\n\
             - Build: `{build_cmd}`\n\
             - Run: `{run_command}`\n\
             {service_section}\n\
             \n\
             ## MCP\n\
             Voir `.claude/rules/mcp-tools.md` pour la liste complete des outils disponibles.\n\
             Tous les outils sont scopes automatiquement a ce projet.\n\
             \n\
             ## Autres apps dans l'environnement\n\
             {other_apps}\n",
            name = app.name,
            env_label = env_label,
            slug = app.slug,
            env_slug = self.env_slug,
            stack = app.stack.display_name(),
            url = url,
            port = app.port,
            structure = structure_section,
            db_section = db_section,
            build_cmd = build_cmd,
            run_command = app.run_command,
            service_section = service_section,
            other_apps = other_apps_section,
        )
    }

    fn render_global_claude_md(&self, apps: &[EnvAgentAppConfig]) -> String {
        let env_label = env_type_label(self.env_type);

        let mut table_rows = String::new();
        for app in apps {
            table_rows.push_str(&format!(
                "| {name} | {slug} | {port} | {stack} | {db} |\n",
                name = app.name,
                slug = app.slug,
                port = app.port,
                stack = app.stack.display_name(),
                db = if app.has_db { "oui" } else { "-" },
            ));
        }

        let env_behavior = if self.env_type == EnvType::Development {
            "- Service watch: `{slug}-watch.service` — rebuild continu en arriere-plan\n  \
             Les modifications de code declenchent un rebuild + restart automatique"
                .to_string()
        } else {
            "- Deploye via pipeline. Ne pas modifier le code directement.".to_string()
        };

        format!(
            "# Environnement {env_slug} — {env_label}\n\
             \n\
             ## Apps dans cet environnement\n\
             | App | Slug | Port | Stack | DB |\n\
             | --- | --- | --- | --- | --- |\n\
             {table_rows}\
             \n\
             ## Fonctionnement\n\
             - Chaque app est TOUJOURS buildee puis servie, meme en dev\n\
             - Service principal: `{{slug}}.service` — sert l'app buildee sur son port\n\
             {env_behavior}\n\
             \n\
             ## Stacks supportees\n\
             - **Next.js** : `pnpm build` puis `node server.js` (custom server + WS)\n\
             - **Axum+Vite** : `cargo build --release` + `vite build` puis binaire Axum sert API + dist/\n\
             - **Axum** : `cargo build --release` puis binaire Axum (API-only, pas de frontend)\n\
             \n\
             ## Regles\n\
             - Utiliser les outils MCP (app.*, db.*, studio.*) — JAMAIS ssh/scp/machinectl\n\
             - Ports 3001+ pour les apps, 4010 pour MCP, 8443 pour code-server\n\
             - Ne pas acceder aux fichiers .db directement — utiliser MCP db.*\n\
             - Voir .claude/rules/env-rules.md pour les permissions detaillees\n",
            env_slug = self.env_slug,
            env_label = env_label,
            table_rows = table_rows,
            env_behavior = env_behavior,
        )
    }

    /// Settings for a specific app (single MCP server with ?project= scoping).
    fn render_settings_json_for_app(&self, app_slug: &str) -> String {
        let settings = serde_json::json!({
            "mcpServers": {
                "env": {
                    "type": "http",
                    "url": format!("http://localhost:{}/mcp?project={}", self.mcp_port, app_slug)
                }
            }
        });
        serde_json::to_string_pretty(&settings).expect("JSON serialization cannot fail")
    }

    /// Settings for global context (no project scoping, fallback).
    fn render_settings_json_global(&self) -> String {
        let settings = serde_json::json!({
            "mcpServers": {
                "env": {
                    "type": "http",
                    "url": format!("http://localhost:{}/mcp", self.mcp_port)
                }
            }
        });
        serde_json::to_string_pretty(&settings).expect("JSON serialization cannot fail")
    }

    /// Render .mcp.json for Claude Code VS Code extension (per-app, project-scoped).
    fn render_mcp_json_for_app(&self, app_slug: &str) -> String {
        let mcp = serde_json::json!({
            "mcpServers": {
                "env": {
                    "type": "http",
                    "url": format!("http://localhost:{}/mcp?project={}", self.mcp_port, app_slug)
                }
            }
        });
        serde_json::to_string_pretty(&mcp).expect("JSON serialization cannot fail")
    }

    fn render_env_rules_md(&self) -> String {
        let perms = EnvPermissions::for_type(self.env_type);
        let env_label = env_type_label(self.env_type);

        let mut lines = Vec::new();
        lines.push(format!(
            "# Regles globales — Environnement {}\n",
            env_label
        ));
        lines.push(
            "Cet environnement est un conteneur nspawn gere par HomeRoute.".to_string(),
        );
        lines.push(
            "Toutes les apps tournent comme services systemd geres par env-agent.\n".to_string(),
        );

        lines.push("| Permission | Autorise |".to_string());
        lines.push("| --- | --- |".to_string());
        lines.push(format!(
            "| Modifier le code source | {} |",
            if perms.code_edit { "Oui" } else { "Non" }
        ));
        lines.push(format!(
            "| Build et run | {} |",
            if perms.build_run { "Oui" } else { "Non" }
        ));
        lines.push(format!(
            "| Modifier le schema DB | {} |",
            if perms.db_schema_write { "Oui" } else { "Non" }
        ));
        lines.push(format!(
            "| Ecrire des donnees DB | {} |",
            if perms.db_data_write { "Oui" } else { "Non" }
        ));
        lines.push(format!(
            "| Lire des donnees DB | {} |",
            if perms.db_data_read { "Oui" } else { "Non" }
        ));
        lines.push(format!(
            "| Lire les logs | {} |",
            if perms.logs_read { "Oui" } else { "Non" }
        ));
        lines.push(format!(
            "| Promouvoir via pipeline | {} |",
            if perms.pipeline_promote { "Oui" } else { "Non" }
        ));
        lines.push(format!(
            "| Rollback | {} |",
            if perms.pipeline_rollback { "Oui" } else { "Non" }
        ));
        lines.push(format!(
            "| Modifier les variables d'env | {} |",
            if perms.env_vars_write { "Oui" } else { "Non" }
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
        lines.push("## Conventions".to_string());
        lines.push(
            "- Ne pas acceder aux fichiers .db directement — utiliser les outils MCP `db.*`"
                .to_string(),
        );
        lines.push(
            "- Pour deployer vers production, utiliser les pipelines".to_string(),
        );
        lines.push(
            "- Utiliser les outils MCP `app.*` pour gerer les services (pas systemctl directement)"
                .to_string(),
        );
        lines.push(
            "- Les modifications de code en dev sont automatiquement rebuilees par le service watch"
                .to_string(),
        );

        lines.push(String::new());
        lines.join("\n")
    }

    fn render_mcp_tools_md(&self, app: &EnvAgentAppConfig) -> String {
        format!(
            "# Outils MCP — {name}\n\
             \n\
             Tous les outils sont automatiquement scopes a ce projet ({slug}).\n\
             Tu n'as PAS besoin de passer app_id, context ou slug.\n\
             \n\
             ## Base de donnees (db.*)\n\
             - `db.list_tables` — lister les tables\n\
             - `db.describe_table` — schema d'une table\n\
             - `db.query_data` — requeter avec filtres, tri, pagination\n\
             - `db.insert_data` — inserer des lignes\n\
             - `db.update_data` — mettre a jour avec filtres\n\
             - `db.delete_data` — supprimer avec filtres\n\
             - `db.create_table` — creer une table\n\
             - `db.add_column` — ajouter une colonne\n\
             - `db.remove_column` — supprimer une colonne\n\
             - `db.get_schema` — schema complet en JSON\n\
             \n\
             ## App (app.*)\n\
             - `app.status` — verifier si le service tourne\n\
             - `app.restart` — redemarrer apres modification\n\
             - `app.logs` — consulter les logs recents\n\
             - `app.health` — health check HTTP\n\
             \n\
             ## Todos (todos.*)\n\
             - `todos.list` — lister les todos du projet\n\
             - `todos.create` — creer un todo\n\
             - `todos.complete` — marquer termine\n\
             - `todos.update` — mettre a jour\n\
             - `todos.delete` — supprimer\n\
             \n\
             ## Pipeline (pipeline.*)\n\
             - `pipeline.promote` — deployer vers l'env suivant\n\
             - `pipeline.status` — statut d'un deploiement\n\
             - `pipeline.history` — historique des deploiements\n\
             \n\
             ## Documentation (docs.*)\n\
             - `docs.get` — lire la doc du projet\n\
             - `docs.update` — mettre a jour une section (meta, structure, features, backend, notes)\n\
             - `docs.completeness` — verifier les sections remplies\n\
             \n\
             ## Git (git.*)\n\
             - `git.log` — derniers commits\n\
             - `git.branches` — lister les branches\n\
             \n\
             ## Secrets (secrets.*)\n\
             - `secrets.list` — lister les variables d'env\n\
             - `secrets.get` — lire une variable\n\
             - `secrets.set` — definir une variable\n\
             - `secrets.delete` — supprimer une variable\n\
             \n\
             ## Jobs (jobs.*)\n\
             - `jobs.create` — lancer un job en arriere-plan\n\
             - `jobs.list` — lister les jobs\n\
             - `jobs.get` — details d'un job\n\
             - `jobs.complete` — marquer un job termine\n",
            name = app.name,
            slug = app.slug,
        )
    }

    fn render_workflow_md(&self, app: &EnvAgentAppConfig) -> String {
        let build_cmd = app
            .build_command
            .as_deref()
            .unwrap_or_else(|| app.stack.default_build_command());

        let env_section = if self.env_type == EnvType::Development {
            format!(
                "- Watch: `{slug}-watch.service` (rebuild continu)\n\
                 \n\
                 ## Developpement\n\
                 - Les modifications de code sont rebuildes automatiquement par le service watch\n\
                 - Verifier les logs: `app.logs`\n\
                 - Verifier la sante: `app.health`\n\
                 - Redemarrer manuellement: `app.restart`\n\
                 \n\
                 ## Deploiement\n\
                 - `pipeline.promote` pour deployer vers acceptance ou production\n\
                 - `pipeline.status` pour suivre le deploiement\n\
                 - `pipeline.history` pour l'historique",
                slug = app.slug,
            )
        } else {
            let label = env_type_label(self.env_type);
            format!(
                "\n\
                 ## Environnement {label}\n\
                 - Code en lecture seule. Ne pas modifier le code directement.\n\
                 - Deploye via pipeline uniquement.",
            )
        };

        format!(
            "# Workflow — {name} ({stack})\n\
             \n\
             ## Service systemd\n\
             - Service: `{slug}.service`\n\
             - Run: `{run_command}`\n\
             - Build: `{build_cmd}`\n\
             {env_section}\n\
             \n\
             ## Base de donnees\n\
             - Utiliser `db.*` pour toutes les operations\n\
             - Ne JAMAIS acceder aux fichiers .db directement\n\
             - Faire un snapshot avant les modifications de schema: `db.snapshot`\n\
             \n\
             ## Todos\n\
             - Consulter `todos.list` en debut de session\n\
             - Creer des todos pour le travail de suivi: `todos.create`\n\
             - Marquer complete quand termine: `todos.complete`\n\
             \n\
             ## Documentation\n\
             - Lire la doc avant de modifier l'app: `docs.get`\n\
             - Mettre a jour apres modification: `docs.update`\n",
            name = app.name,
            stack = app.stack.display_name(),
            slug = app.slug,
            run_command = app.run_command,
            build_cmd = build_cmd,
            env_section = env_section,
        )
    }
}

// ── Helpers ────────────────────────────────────────────────────────────

fn env_type_label(env_type: EnvType) -> &'static str {
    match env_type {
        EnvType::Development => "Development",
        EnvType::Acceptance => "Acceptance",
        EnvType::Production => "Production",
    }
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
    use hr_environment::types::AppStackType;

    fn test_generator() -> ContextGenerator {
        ContextGenerator {
            env_slug: "dev".to_string(),
            env_type: EnvType::Development,
            base_domain: "mynetwk.biz".to_string(),
            apps_path: "/tmp/ctx-test-apps".to_string(),
            mcp_port: 4010,
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
            watch_command: None,
            test_command: None,
            description: None,
        }
    }

    fn test_app_wallet() -> EnvAgentAppConfig {
        EnvAgentAppConfig {
            slug: "wallet".to_string(),
            name: "Wallet".to_string(),
            stack: AppStackType::AxumVite,
            port: 3002,
            run_command: "./bin/wallet".to_string(),
            build_command: None,
            health_path: "/api/health".to_string(),
            has_db: false,
            watch_command: None,
            test_command: None,
            description: None,
        }
    }

    #[test]
    fn test_generate_creates_files() {
        let ctx = test_generator();
        let app = test_app();
        let all_apps = vec![app.clone()];
        let base = PathBuf::from(&ctx.apps_path);

        // Clean up from previous runs
        let _ = fs::remove_dir_all(&base);

        let tables = vec!["users".to_string(), "trades".to_string()];
        ctx.generate_for_app(&app, &all_apps, Some(tables)).unwrap();

        let claude_md = fs::read_to_string(base.join("trader/CLAUDE.md")).unwrap();
        assert!(claude_md.contains("# Trader"));
        assert!(claude_md.contains("slug: trader"));
        assert!(claude_md.contains("Axum + Vite/React"));
        assert!(claude_md.contains("`users`"));
        assert!(claude_md.contains("`trades`"));
        assert!(claude_md.contains("cargo build --release"));
        assert!(claude_md.contains("Service principal:"));
        assert!(claude_md.contains("mcp-tools.md"));

        // Single MCP server with project scoping
        let settings =
            fs::read_to_string(base.join("trader/.claude/settings.json")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&settings).unwrap();
        let env_url = parsed["mcpServers"]["env"]["url"].as_str().unwrap();
        assert!(env_url.contains("4010"));
        assert!(env_url.contains("?project=trader"));
        // Only 1 MCP server now
        assert_eq!(parsed["mcpServers"].as_object().unwrap().len(), 1);

        // Per-app rules files should exist
        assert!(base.join("trader/.claude/rules/env-rules.md").exists());
        assert!(base.join("trader/.claude/rules/mcp-tools.md").exists());
        assert!(base.join("trader/.claude/rules/workflow.md").exists());

        // Clean up
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_no_db() {
        let ctx = test_generator();
        let mut app = test_app();
        app.has_db = false;

        let md = ctx.render_claude_md(&app, &[app.clone()], &None);
        assert!(md.contains("Pas de base de donnees configuree"));
    }

    #[test]
    fn test_prod_permissions() {
        let ctx = ContextGenerator {
            env_type: EnvType::Production,
            ..test_generator()
        };

        let rules_md = ctx.render_env_rules_md();
        assert!(rules_md.contains("PRODUCTION"));
        assert!(rules_md.contains("| Modifier le code source | Non |"));
        assert!(rules_md.contains("| Rollback | Oui |"));
        assert!(rules_md.contains("## Conventions"));
    }

    #[test]
    fn test_no_build_command() {
        let ctx = test_generator();
        let mut app = test_app();
        app.build_command = None;

        let md = ctx.render_claude_md(&app, &[app.clone()], &None);
        // Should fall back to stack default build command
        assert!(md.contains(app.stack.default_build_command()));
    }

    #[test]
    fn test_other_apps_listed() {
        let ctx = test_generator();
        let trader = test_app();
        let wallet = test_app_wallet();
        let all_apps = vec![trader.clone(), wallet.clone()];

        let md = ctx.render_claude_md(&trader, &all_apps, &None);
        assert!(md.contains("Autres apps dans l'environnement"));
        assert!(md.contains("Wallet (wallet)"));
        assert!(md.contains("port 3002"));

        // Wallet's CLAUDE.md should list Trader
        let md2 = ctx.render_claude_md(&wallet, &all_apps, &None);
        assert!(md2.contains("Trader (trader)"));
    }

    #[test]
    fn test_description_overrides_structure() {
        let ctx = test_generator();
        let mut app = test_app();
        app.description = Some("Custom app description here".to_string());

        let md = ctx.render_claude_md(&app, &[app.clone()], &None);
        assert!(md.contains("Custom app description here"));
        assert!(!md.contains(app.stack.project_structure()));
    }

    #[test]
    fn test_dev_watch_section() {
        let ctx = test_generator();
        let app = test_app();

        let md = ctx.render_claude_md(&app, &[app.clone()], &None);
        assert!(md.contains("watch.service"));
        assert!(md.contains("rebuild continu"));
    }

    #[test]
    fn test_prod_no_watch() {
        let ctx = ContextGenerator {
            env_type: EnvType::Production,
            ..test_generator()
        };
        let app = test_app();

        let md = ctx.render_claude_md(&app, &[app.clone()], &None);
        assert!(!md.contains("watch.service"));
        assert!(md.contains("Deploye via pipeline"));
    }

    #[test]
    fn test_refresh_all() {
        let ctx = ContextGenerator {
            apps_path: "/tmp/ctx-test-refresh".to_string(),
            ..test_generator()
        };

        let _ = fs::remove_dir_all(&ctx.apps_path);

        let apps = vec![test_app(), test_app_wallet()];

        ctx.refresh_all(&apps).unwrap();

        assert!(PathBuf::from("/tmp/ctx-test-refresh/trader/CLAUDE.md").exists());
        assert!(PathBuf::from("/tmp/ctx-test-refresh/wallet/CLAUDE.md").exists());

        let _ = fs::remove_dir_all(&ctx.apps_path);
    }

    #[test]
    fn test_generate_global_context() {
        let ctx = ContextGenerator {
            apps_path: "/tmp/ctx-test-global".to_string(),
            ..test_generator()
        };

        let _ = fs::remove_dir_all(&ctx.apps_path);

        let apps = vec![test_app(), test_app_wallet()];
        ctx.generate_global_context(&apps).unwrap();

        let root = PathBuf::from(&ctx.apps_path);

        // Check CLAUDE.md
        let claude_md = fs::read_to_string(root.join("CLAUDE.md")).unwrap();
        assert!(claude_md.contains("Environnement dev"));
        assert!(claude_md.contains("| Trader |"));
        assert!(claude_md.contains("| Wallet |"));
        assert!(claude_md.contains("oui")); // trader has_db
        assert!(claude_md.contains("Stacks supportees"));

        // Check settings.json (single MCP server, no project scoping)
        let settings = fs::read_to_string(root.join(".claude/settings.json")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&settings).unwrap();
        let env_url = parsed["mcpServers"]["env"]["url"].as_str().unwrap();
        assert!(env_url.contains("4010"));
        assert!(!env_url.contains("?project="), "global settings should not have project scoping");
        assert_eq!(parsed["mcpServers"].as_object().unwrap().len(), 1);

        // Check env-rules.md
        let rules = fs::read_to_string(root.join(".claude/rules/env-rules.md")).unwrap();
        assert!(rules.contains("Regles globales"));
        assert!(rules.contains("conteneur nspawn"));
        assert!(rules.contains("Conventions"));
        assert!(rules.contains("| Modifier le code source | Oui |"));

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

    #[test]
    fn test_render_settings_json_for_app() {
        let ctx = test_generator();
        let json = ctx.render_settings_json_for_app("trader");
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let url = parsed["mcpServers"]["env"]["url"].as_str().unwrap();
        assert_eq!(url, "http://localhost:4010/mcp?project=trader");
        assert_eq!(parsed["mcpServers"].as_object().unwrap().len(), 1);
    }

    #[test]
    fn test_render_settings_json_global() {
        let ctx = test_generator();
        let json = ctx.render_settings_json_global();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let url = parsed["mcpServers"]["env"]["url"].as_str().unwrap();
        assert_eq!(url, "http://localhost:4010/mcp");
        assert!(!url.contains("?project="));
    }

    #[test]
    fn test_render_mcp_tools_md() {
        let ctx = test_generator();
        let app = test_app();
        let md = ctx.render_mcp_tools_md(&app);
        assert!(md.contains("# Outils MCP — Trader"));
        assert!(md.contains("scopes a ce projet (trader)"));
        assert!(md.contains("## Base de donnees (db.*)"));
        assert!(md.contains("db.list_tables"));
        assert!(md.contains("## App (app.*)"));
        assert!(md.contains("app.restart"));
        assert!(md.contains("## Todos (todos.*)"));
        assert!(md.contains("## Pipeline (pipeline.*)"));
        assert!(md.contains("## Documentation (docs.*)"));
        assert!(md.contains("## Git (git.*)"));
        assert!(md.contains("## Secrets (secrets.*)"));
        assert!(md.contains("## Jobs (jobs.*)"));
    }

    #[test]
    fn test_render_workflow_md_dev() {
        let ctx = test_generator();
        let app = test_app();
        let md = ctx.render_workflow_md(&app);
        assert!(md.contains("# Workflow — Trader (Axum + Vite/React)"));
        assert!(md.contains("trader.service"));
        assert!(md.contains("trader-watch.service"));
        assert!(md.contains("## Developpement"));
        assert!(md.contains("## Deploiement"));
        assert!(md.contains("pipeline.promote"));
        assert!(md.contains("## Base de donnees"));
        assert!(md.contains("## Todos"));
        assert!(md.contains("## Documentation"));
    }

    #[test]
    fn test_render_workflow_md_prod() {
        let ctx = ContextGenerator {
            env_type: EnvType::Production,
            ..test_generator()
        };
        let app = test_app();
        let md = ctx.render_workflow_md(&app);
        assert!(md.contains("# Workflow — Trader"));
        assert!(!md.contains("watch.service"));
        assert!(md.contains("Environnement Production"));
        assert!(md.contains("lecture seule"));
        assert!(md.contains("pipeline uniquement"));
    }
}

//! Claude Code context generation for per-app Studio awareness.
//!
//! Generates per-app context files so that Claude Code (running in code-server
//! or via the VS Code extension) has full project awareness:
//!   - {apps_path}/{slug}/CLAUDE.md                  — project identity, DB schema, MCP usage
//!   - {apps_path}/{slug}/.claude/settings.json      — MCP server + auto-approve permissions
//!   - {apps_path}/{slug}/.claude/rules/mcp-tools.md — MCP tools documentation
//!   - {apps_path}/{slug}/.claude/rules/workflow.md  — development workflow
//!   - {apps_path}/{slug}/.mcp.json                  — MCP server config (CLI compat)
//!
//! Also generates global workspace files at the apps root:
//!   - {apps_path}/CLAUDE.md                  — workspace overview, all apps table
//!   - {apps_path}/.claude/settings.json      — MCP server (no project scoping)
//!   - {apps_path}/.mcp.json                  — MCP server config (CLI compat)

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use tracing::{info, warn};

use crate::types::{Application, Visibility};

/// Generates Claude Code context files for HomeRoute apps.
pub struct ContextGenerator {
    pub apps_path: PathBuf,
    pub base_domain: String,
    pub mcp_endpoint: String,
    pub mcp_token: Option<String>,
}

impl ContextGenerator {
    pub fn new(
        apps_path: impl Into<PathBuf>,
        base_domain: impl Into<String>,
        mcp_endpoint: impl Into<String>,
    ) -> Self {
        let mcp_token = std::env::var("MCP_TOKEN").ok();
        Self {
            apps_path: apps_path.into(),
            base_domain: base_domain.into(),
            mcp_endpoint: mcp_endpoint.into(),
            mcp_token,
        }
    }

    /// Generate all context files for a single app. Idempotent.
    pub fn generate_for_app(
        &self,
        app: &Application,
        all_apps: &[Application],
        db_tables: Option<Vec<String>>,
    ) -> anyhow::Result<()> {
        let app_dir = self.apps_path.join(&app.slug);
        let claude_dir = app_dir.join(".claude");
        let rules_dir = claude_dir.join("rules");
        fs::create_dir_all(&rules_dir)?;

        let claude_md = self.render_claude_md(app, all_apps, &db_tables);
        log_write(&app.slug, &app_dir.join("CLAUDE.md"), &claude_md)?;

        // Project-scoped MCP endpoint: ?project={slug} pre-contextualizes all tools
        let project_mcp = format!("{}?project={}", self.mcp_endpoint, app.slug);
        let settings = render_settings_json_with_auth(&project_mcp, self.mcp_token.as_deref());
        log_write(&app.slug, &claude_dir.join("settings.json"), &settings)?;

        let mcp_json = render_mcp_json_with_auth(&project_mcp, self.mcp_token.as_deref());
        log_write(&app.slug, &app_dir.join(".mcp.json"), &mcp_json)?;

        // Also write into src/ (where code-server opens projects)
        let src_dir = app.src_dir();
        if src_dir.exists() {
            let src_claude_dir = src_dir.join(".claude");
            fs::create_dir_all(&src_claude_dir)?;
            log_write(&app.slug, &src_dir.join(".mcp.json"), &mcp_json)?;
            log_write(&app.slug, &src_claude_dir.join("settings.json"), &settings)?;

            // Generate skills (slash commands) based on app stack
            let commands_dir = src_claude_dir.join("commands");
            fs::create_dir_all(&commands_dir)?;
            for (name, content) in render_skills(app) {
                log_write(&app.slug, &commands_dir.join(format!("{name}.md")), &content)?;
            }
        }

        let mcp_tools = render_mcp_tools_md(app);
        log_write(&app.slug, &rules_dir.join("mcp-tools.md"), &mcp_tools)?;

        let workflow = self.render_workflow_md(app);
        log_write(&app.slug, &rules_dir.join("workflow.md"), &workflow)?;

        // Clean up legacy rule files from the env-agent era.
        for legacy in &[
            "deploy.md",
            "env-context.md",
            "env-rules.md",
            "git.md",
            "homeroute-deploy.md",
            "homeroute-dev.md",
            "homeroute-docs.md",
            "homeroute-dataverse.md",
            "homeroute-store.md",
            "project.md",
        ] {
            let p = rules_dir.join(legacy);
            if p.exists() {
                let _ = fs::remove_file(&p);
            }
        }

        info!(slug = %app.slug, "context files generated");
        let _ = db_tables;
        Ok(())
    }

    /// Generate the workspace-root context files (CLAUDE.md, settings.json, .mcp.json).
    pub fn generate_root(&self, all_apps: &[Application]) -> anyhow::Result<()> {
        let claude_dir = self.apps_path.join(".claude");
        fs::create_dir_all(&claude_dir)?;

        let claude_md = self.render_root_claude_md(all_apps);
        log_write("<root>", &self.apps_path.join("CLAUDE.md"), &claude_md)?;

        let settings = render_settings_json_with_auth(&self.mcp_endpoint, self.mcp_token.as_deref());
        log_write("<root>", &claude_dir.join("settings.json"), &settings)?;

        let mcp_json = render_mcp_json_with_auth(&self.mcp_endpoint, self.mcp_token.as_deref());
        log_write("<root>", &self.apps_path.join(".mcp.json"), &mcp_json)?;

        info!(count = all_apps.len(), "workspace-root context written");
        Ok(())
    }

    /// Refresh per-app context for every app + workspace-root context.
    pub fn refresh_all(&self, all_apps: &[Application]) -> anyhow::Result<()> {
        for app in all_apps {
            if let Err(e) = self.generate_for_app(app, all_apps, None) {
                warn!(slug = %app.slug, error = %e, "failed to generate app context");
            }
        }
        if let Err(e) = self.generate_root(all_apps) {
            warn!(error = %e, "failed to generate root context");
        }
        Ok(())
    }

    // ── Renderers ──────────────────────────────────────────────────────

    fn render_claude_md(
        &self,
        app: &Application,
        all_apps: &[Application],
        db_tables: &Option<Vec<String>>,
    ) -> String {
        let url = format!("https://{}", app.domain);

        let visibility_label = match app.visibility {
            Visibility::Public => "Public (no auth required)",
            Visibility::Private => "Private (HomeRoute auth required)",
        };

        let db_section = match (app.has_db, db_tables) {
            (true, Some(tables)) if !tables.is_empty() => {
                let mut s = String::from("Managed SQLite database (Dataverse).\n\n**Tables :**\n");
                for t in tables {
                    s.push_str(&format!("- `{}`\n", t));
                }
                s.push_str("\n- Path: `");
                s.push_str(&app.db_path().display().to_string());
                s.push_str("`\n");
                s.push_str(
                    "- Use the MCP tools `db.*` — never open the .db file directly.\n\
                     - Read: `db_tables`, `db_schema`, `db_get_schema`, `db_overview`, `db_count_rows`, `db_query`\n\
                     - Mutate data: `db_exec` (INSERT/UPDATE/DELETE)\n\
                     - Mutate schema: `db_create_table`, `db_drop_table`, `db_add_column`, `db_remove_column`, `db_create_relation`, `db_sync_schema`\n\
                     - Maintenance: `db_snapshot` (timestamped backup)\n\
                     - Always declare FK relations via `db_create_relation` to enable automatic JOIN expansion.\n",
                );
                s
            }
            (true, _) => format!(
                "Managed SQLite database (Dataverse, tables not yet inspected).\n\n\
                 - Path: `{}`\n\
                 - Use the MCP tools `db.*` — never open the .db file directly.\n",
                app.db_path().display(),
            ),
            (false, _) => "No database configured for this app.".to_string(),
        };

        let structure_section = match &app.description {
            Some(desc) if !desc.trim().is_empty() => desc.clone(),
            _ => format!(
                "- Sources: `{}`\n- Build artifacts and the `.env` file live alongside `src/`.",
                app.src_dir().display()
            ),
        };

        let build_cmd = app.build_command.as_deref().unwrap_or("(no build step)");

        let env_var_section = if app.env_vars.is_empty() {
            "No custom environment variables declared.".to_string()
        } else {
            let mut s = String::from("Defined for this app (values are not shown):\n");
            for k in app.env_vars.keys() {
                s.push_str(&format!("- `{}`\n", k));
            }
            s.push_str(
                "\nThe runtime always sees `PORT` (the app must listen on this port — never hardcode).\n",
            );
            s
        };

        let other_apps: Vec<String> = all_apps
            .iter()
            .filter(|a| a.slug != app.slug)
            .map(|a| {
                format!(
                    "- {name} (`{slug}`) — {stack}, https://{domain}",
                    name = a.name,
                    slug = a.slug,
                    stack = a.stack.display_name(),
                    domain = a.domain,
                )
            })
            .collect();

        let other_apps_section = if other_apps.is_empty() {
            "No other apps configured.".to_string()
        } else {
            other_apps.join("\n")
        };

        format!(
            "# {name}\n\
             \n\
             ## Identity\n\
             - **Name:** {name}\n\
             - **Slug:** `{slug}`\n\
             - **Stack:** {stack}\n\
             - **URL:** {url}\n\
             - **Visibility:** {visibility}\n\
             - **Port:** {port}\n\
             - **App directory:** `{app_dir}`\n\
             \n\
             ## Project structure\n\
             {structure}\n\
             \n\
             ## Database\n\
             {db_section}\n\
             \n\
             ## Build & run\n\
             - **Build:** `{build_cmd}`\n\
             - **Run:** `{run_command}`\n\
             - **Health path:** `{health_path}`\n\
             - Edit sources in `{src_dir}`, then build on place, restart via MCP `app.control` \
             (or `POST /api/apps/{slug}/control` with `{{\"action\":\"restart\"}}`), and verify on {url}.\n\
             \n\
             ## Regles de developpement\n\
             - **Toujours builder sur place** : compiler directement sur le serveur de production, jamais de cross-compile depuis un autre poste.\n\
             - **Pas de mode dev** : pas de `pnpm dev`, `npm run dev`, ou watch mode. Toujours builder pour la production (`pnpm build`, `cargo build --release`).\n\
             - **Pas de pipelines** : pas de chaine de promotion dev→acc→prod. Le workflow est : editer → builder → restart → verifier.\n\
             \n\
             ## Environment variables\n\
             {env_vars}\n\
             \n\
             ## MCP\n\
             A single MCP server (`homeroute`) is wired up via `.claude/settings.json` and \
             `.mcp.json`. See `.claude/rules/mcp-tools.md` for the full tool list.\n\
             \n\
             Read-only tools (`app.list`, `app.status`, `app.logs`, `db.tables`, `db.schema`, \
             `db.query`, `db.snapshot`, `docs.get`, `docs.list`, `docs.search`) are auto-approved. \
             Mutations (delete, schema writes, doc updates) require explicit confirmation.\n\
             \n\
             ## Logging\n\
             - Use structured logging (`info!(field = value, \"message\")`) for every meaningful \
             operation: HTTP handlers, IPC calls, config writes, errors.\n\
             - Never log secrets, tokens or full request bodies.\n\
             - Logs are visible via `app.logs` and the HomeRoute logs page.\n\
             \n\
             ## Other apps in this workspace\n\
             {other_apps}\n",
            name = app.name,
            slug = app.slug,
            stack = app.stack.display_name(),
            url = url,
            visibility = visibility_label,
            port = app.port,
            app_dir = app.app_dir().display(),
            structure = structure_section,
            db_section = db_section,
            build_cmd = build_cmd,
            run_command = app.run_command,
            health_path = app.health_path,
            src_dir = app.src_dir().display(),
            env_vars = env_var_section,
            other_apps = other_apps_section,
        )
    }

    fn render_root_claude_md(&self, all_apps: &[Application]) -> String {
        let mut table_rows = String::new();
        for app in all_apps {
            let db_cell = if app.has_db {
                format!("`{}`", app.db_path().display())
            } else {
                "—".to_string()
            };
            let visibility = match app.visibility {
                Visibility::Public => "public",
                Visibility::Private => "private",
            };
            table_rows.push_str(&format!(
                "| {name} | `{slug}` | {stack} | https://{domain} | {visibility} | {db} |\n",
                name = app.name,
                slug = app.slug,
                stack = app.stack.display_name(),
                domain = app.domain,
                visibility = visibility,
                db = db_cell,
            ));
        }

        if table_rows.is_empty() {
            table_rows.push_str("| _no apps yet_ |  |  |  |  |  |\n");
        }

        format!(
            "# HomeRoute Apps Workspace\n\
             \n\
             This is the workspace root for every application managed by HomeRoute. \
             Each app lives under `{apps_path}/<slug>/` with its own sources, build \
             artifacts, `.env` and (optionally) managed SQLite DB.\n\
             \n\
             ## Apps\n\
             | Name | Slug | Stack | URL | Visibility | DB path |\n\
             | --- | --- | --- | --- | --- | --- |\n\
             {table_rows}\
             \n\
             ## How HomeRoute runs apps\n\
             - Apps run **directly on the host** as processes supervised by HomeRoute \
             (no nspawn container, no env-agent).\n\
             - The reverse proxy `hr-edge` terminates TLS on `*.{base_domain}` and forwards to \
             each app's local port.\n\
             - The orchestrator manages the process lifecycle (start, stop, restart, logs, \
             health) and exposes everything via MCP.\n\
             \n\
             ## Working in this workspace\n\
             - Open any `<slug>/` subdirectory to focus on a single app — its `.claude/` \
             folder will scope Claude Code to that project.\n\
             - From this root, use the MCP tool `app.list` to enumerate apps, then \
             `app.status` / `app.logs` / `app.restart` to operate on them.\n\
             - Edit sources in `<slug>/src/`, then `app.restart <slug>` and verify on the \
             public URL.\n\
             \n\
             ## MCP\n\
             A single MCP server `homeroute` is configured at `{mcp_endpoint}` via \
             `.claude/settings.json` and `.mcp.json`. Read-only tools (`app.list`, \
             `app.status`, `app.logs`, `db.tables`, `db.schema`, `db.query`, `db.snapshot`, \
             `docs.*`) are auto-approved.\n\
             \n\
             ## Rules\n\
             - Never use `ssh`, `scp` or direct filesystem access on `*.db` files — go \
             through the MCP `db.*` tools.\n\
             - Apps must read their listening port from `PORT`, never hardcode it.\n\
             - Update each app's docs (`docs.update`) after meaningful changes.\n",
            apps_path = self.apps_path.display(),
            table_rows = table_rows,
            base_domain = self.base_domain,
            mcp_endpoint = self.mcp_endpoint,
        )
    }

    fn render_workflow_md(&self, app: &Application) -> String {
        let build_cmd = app.build_command.as_deref().unwrap_or("(no build step)");
        let url = format!("https://{}", app.domain);

        format!(
            "# Workflow — {name} ({stack})\n\
             \n\
             ## Process\n\
             - **Run:** `{run_command}`\n\
             - **Build:** `{build_cmd}`\n\
             - **Health:** `{health_path}`\n\
             - **Public URL:** {url}\n\
             - Managed by HomeRoute as a host-level process. Use MCP `app.*` tools to \
             control it — never invoke `systemctl` or `kill` directly.\n\
             \n\
             ## Edit → build → restart → verify\n\
             1. Edit sources in `{src_dir}`.\n\
             2. Build on place : `{build_cmd}` (toujours en production, jamais de mode dev).\n\
             3. Restart via MCP: `app.control` (ou `POST /api/apps/{slug}/control` avec `{{\"action\":\"restart\"}}`).\n\
             4. Check the result via `app.status` and `app.logs`.\n\
             5. Open {url} to validate the change end-to-end.\n\
             \n\
             ## Regles\n\
             - **Builder sur place** : jamais de cross-compile, tout se compile sur le serveur de production.\n\
             - **Pas de mode dev** : pas de `pnpm dev` / `cargo watch`. Production only.\n\
             - **Pas de pipelines** : pas de promotion dev→acc→prod.\n\
             \n\
             ## Environment variables\n\
             The orchestrator injects:\n\
             - `PORT` — listen on this port. **Never hardcode** a port in the code.\n\
             - `DATABASE_URL` / `DATABASE_PATH` — path to the managed SQLite DB \
             (only when `has_db` is true).\n\
             - Any custom variables declared on the application (managed via the API).\n\
             \n\
             ## Database\n\
             - Use the MCP `db.*` tools for every read/write — they target the managed DB \
             for this app automatically.\n\
             - Take a snapshot with `db.snapshot` before any schema change.\n\
             - Never open the `.db` file by hand.\n\
             \n\
             ## Documentation\n\
             - Always read the existing docs with `docs.get` before non-trivial changes.\n\
             - After a feature, structural change or backend tweak, update the relevant \
             section with `docs.update`.\n\
             \n\
             ## Logging\n\
             - Add structured log lines for new handlers, IPC calls, errors, and \
             unexpected branches.\n\
             - Inspect logs via `app.logs` and the HomeRoute logs page.\n",
            name = app.name,
            stack = app.stack.display_name(),
            run_command = app.run_command,
            build_cmd = build_cmd,
            health_path = app.health_path,
            url = url,
            src_dir = app.src_dir().display(),
            slug = app.slug,
        )
    }
}

// ── Standalone helpers ─────────────────────────────────────────────────

fn render_mcp_tools_md(app: &Application) -> String {
    format!(
        "# MCP tools — {name}\n\
         \n\
         A single MCP server is configured: `homeroute`. Read-only tools are \
         auto-approved via `.claude/settings.json` — mutations require explicit \
         confirmation.\n\
         \n\
         ## Apps (`app.*`)\n\
         - `app.list` — list every application\n\
         - `app.status` — runtime status of an app (state, port, health)\n\
         - `app.create` — register a new application\n\
         - `app.control` — start / stop / restart\n\
         - `app.exec` — run a one-shot command in the app's context\n\
         - `app.logs` — tail recent logs for an app\n\
         - `app.delete` — remove an application (mutation, not auto-approved)\n\
         \n\
         ## Database (`db.*`)\n\
         - `db.tables` — list tables for `{slug}` (or any app)\n\
         - `db.schema` — describe a table\n\
         - `db.query` — read or mutate via SQL (mutating SQL is not auto-approved)\n\
         - `db.snapshot` — take a snapshot of the managed DB before risky changes\n\
         \n\
         ## Documentation (`docs.*`)\n\
         - `docs.list` — list documented apps and completeness\n\
         - `docs.get` — read a doc section (`meta`, `structure`, `features`, `backend`, `notes`)\n\
         - `docs.search` — full-text search across all docs\n\
         - `docs.update` — update a section (mutation, not auto-approved)\n\
         \n\
         ## Store (`store.*`)\n\
         - Tools for the HomeRoute mobile store (uploads, listings).\n\
         \n\
",
        name = app.name,
        slug = app.slug,
    )
}

fn mcp_server_entry(endpoint: &str, token: Option<&str>) -> serde_json::Value {
    let mut entry = serde_json::json!({
        "type": "http",
        "url": endpoint,
    });
    if let Some(t) = token {
        entry["headers"] = serde_json::json!({
            "Authorization": format!("Bearer {t}")
        });
    }
    entry
}

fn render_settings_json_with_auth(mcp_endpoint: &str, token: Option<&str>) -> String {
    let settings = serde_json::json!({
        "mcpServers": {
            "homeroute": mcp_server_entry(mcp_endpoint, token),
        },
        "enabledMcpjsonServers": ["homeroute"],
        "permissions": {
            "allow": [
                "mcp__homeroute__app_list",
                "mcp__homeroute__app_status",
                "mcp__homeroute__app_logs",
                "mcp__homeroute__db_tables",
                "mcp__homeroute__db_schema",
                "mcp__homeroute__db_query",
                "mcp__homeroute__db_snapshot",
                "mcp__homeroute__db_get_schema",
                "mcp__homeroute__db_sync_schema",
                "mcp__homeroute__db_overview",
                "mcp__homeroute__db_count_rows",
                "mcp__homeroute__docs_get",
                "mcp__homeroute__docs_list",
                "mcp__homeroute__docs_search",
            ],
            "deny": [],
        }
    });
    serde_json::to_string_pretty(&settings).expect("settings JSON serializes")
}

fn render_mcp_json_with_auth(mcp_endpoint: &str, token: Option<&str>) -> String {
    let mcp = serde_json::json!({
        "mcpServers": {
            "homeroute": mcp_server_entry(mcp_endpoint, token),
        }
    });
    serde_json::to_string_pretty(&mcp).expect("mcp JSON serializes")
}

/// Write `content` to `path` only if the existing content differs.
/// Returns `true` if the file was actually written.
fn write_if_changed(path: &Path, content: &str) -> io::Result<bool> {
    if let Ok(existing) = fs::read_to_string(path) {
        if existing == content {
            return Ok(false);
        }
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(true)
}

fn log_write(slug: &str, path: &Path, content: &str) -> io::Result<()> {
    let changed = write_if_changed(path, content)?;
    if changed {
        info!(slug = %slug, file = %path.display(), "context written");
    } else {
        info!(slug = %slug, file = %path.display(), "context unchanged");
    }
    Ok(())
}

fn render_skills(app: &Application) -> Vec<(&'static str, String)> {
    use crate::types::AppStack;
    let build_cmd = app.build_command.as_deref().unwrap_or("echo 'no build command configured'");
    let mut skills = vec![
        ("build", format!(
            "Build the project.\n\n\
             Execute the build command via the `exec` MCP tool:\n\
             ```\n{build_cmd}\n```\n\n\
             Report the result (success or failure with error details)."
        )),
        ("deploy", format!(
            "Build and restart the application.\n\n\
             1. Execute the build command via `exec`:\n   ```\n   {build_cmd}\n   ```\n\
             2. If the build succeeds, call `restart` to restart the process.\n\
             3. Wait 3 seconds, then call `status` to verify the app is running.\n\
             4. Report the result."
        )),
        ("status", "Get the current application status.\n\nCall the `status` MCP tool and display the result concisely: state, PID, port, uptime, restart count.".to_string()),
        ("logs", "Get recent application logs and analyze them.\n\nCall the `logs` MCP tool. Identify any errors or warnings and suggest actions if needed.".to_string()),
    ];

    if app.has_db {
        skills.push(("db-info",
            "Get a summary of the application's database.\n\n\
             1. Call `db_tables` to list all tables.\n\
             2. For each table, call `db_schema` to get the columns and row count.\n\
             3. Display a concise summary: table name, column count, row count.".to_string()
        ));
    }

    match app.stack {
        AppStack::NextJs => {
            skills.push(("install", "Install Node.js dependencies.\n\nExecute `pnpm install` via the `exec` tool. Report any errors.".to_string()));
        }
        AppStack::AxumVite => {
            skills.push(("build-server", format!(
                "Build only the Rust server.\n\nExecute via `exec`:\n```\ncd server && cargo build --release\n```\nReport the result."
            )));
            skills.push(("build-client", "Build only the Vite/React client.\n\nExecute via `exec`:\n```\ncd client && pnpm build\n```\nReport the result.".to_string()));
        }
        AppStack::Axum => {
            skills.push(("build-api", format!(
                "Build the Rust API.\n\nExecute via `exec`:\n```\n{build_cmd}\n```\nReport the result."
            )));
        }
        _ => {}
    }

    skills
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AppStack, Application};
    use std::collections::BTreeMap;

    fn make_app(slug: &str, name: &str, has_db: bool) -> Application {
        let mut app = Application::new(slug.to_string(), name.to_string(), AppStack::AxumVite);
        app.has_db = has_db;
        app.port = 3001;
        app.run_command = format!("./bin/{}", slug);
        app.build_command = Some("cargo build --release".to_string());
        app.health_path = "/api/health".to_string();
        let mut env_vars = BTreeMap::new();
        env_vars.insert("API_KEY".to_string(), "secret".to_string());
        app.env_vars = env_vars;
        app
    }

    fn test_generator(tmp: &Path) -> ContextGenerator {
        ContextGenerator::new(
            tmp.to_path_buf(),
            "mynetwk.biz".to_string(),
            "http://127.0.0.1:4001/mcp".to_string(),
        )
    }

    #[test]
    fn generate_for_app_creates_expected_files() {
        let tmp = std::env::temp_dir().join("hr-apps-context-test-1");
        let _ = fs::remove_dir_all(&tmp);
        let ctx = test_generator(&tmp);
        let trader = make_app("trader", "Trader", true);
        let wallet = make_app("wallet", "Wallet", false);
        let all = vec![trader.clone(), wallet.clone()];

        ctx.generate_for_app(&trader, &all, Some(vec!["users".into(), "trades".into()]))
            .unwrap();

        let claude_md = fs::read_to_string(tmp.join("trader/CLAUDE.md")).unwrap();
        assert!(claude_md.contains("# Trader"));
        assert!(claude_md.contains("`trader`"));
        assert!(claude_md.contains("Axum + Vite/React"));
        assert!(claude_md.contains("`users`"));
        assert!(claude_md.contains("`trades`"));
        assert!(claude_md.contains("Wallet"));
        assert!(claude_md.contains("`API_KEY`"));

        let settings = fs::read_to_string(tmp.join("trader/.claude/settings.json")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&settings).unwrap();
        assert_eq!(
            parsed["mcpServers"]["homeroute"]["url"].as_str().unwrap(),
            "http://127.0.0.1:4001/mcp"
        );
        assert!(
            parsed["permissions"]["allow"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v.as_str() == Some("mcp__homeroute__app_list"))
        );

        let mcp_json = fs::read_to_string(tmp.join("trader/.mcp.json")).unwrap();
        assert!(mcp_json.contains("\"homeroute\""));

        assert!(tmp.join("trader/.claude/rules/mcp-tools.md").exists());
        assert!(tmp.join("trader/.claude/rules/workflow.md").exists());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn generate_root_lists_apps_and_writes_settings() {
        let tmp = std::env::temp_dir().join("hr-apps-context-test-2");
        let _ = fs::remove_dir_all(&tmp);
        let ctx = test_generator(&tmp);
        let trader = make_app("trader", "Trader", true);
        let wallet = make_app("wallet", "Wallet", false);

        ctx.generate_root(&[trader, wallet]).unwrap();

        let root = fs::read_to_string(tmp.join("CLAUDE.md")).unwrap();
        assert!(root.contains("HomeRoute Apps Workspace"));
        assert!(root.contains("| Trader | `trader`"));
        assert!(root.contains("| Wallet | `wallet`"));
        assert!(tmp.join(".claude/settings.json").exists());
        assert!(tmp.join(".mcp.json").exists());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn write_if_changed_skips_when_identical() {
        let tmp = std::env::temp_dir().join("hr-apps-context-test-3");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let path = tmp.join("file.txt");

        assert!(write_if_changed(&path, "hello").unwrap());
        assert!(!write_if_changed(&path, "hello").unwrap());
        assert!(write_if_changed(&path, "world").unwrap());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn no_db_app_renders_no_database_section() {
        let tmp = std::env::temp_dir().join("hr-apps-context-test-4");
        let ctx = test_generator(&tmp);
        let app = make_app("static", "Static", false);
        let md = ctx.render_claude_md(&app, &[app.clone()], &None);
        assert!(md.contains("No database configured"));
    }
}

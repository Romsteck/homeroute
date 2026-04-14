//! Claude Code context generation for per-app Studio awareness.
//!
//! Generates per-app context files so that Claude Code (running in code-server
//! or via the VS Code extension) has full project awareness:
//!   - {apps_path}/{slug}/CLAUDE.md                  — project identity, DB schema, MCP usage
//!   - {apps_path}/{slug}/.claude/settings.json      — MCP server + auto-approve permissions
//!   - {apps_path}/{slug}/.claude/rules/mcp-tools.md — MCP tools documentation
//!   - {apps_path}/{slug}/.claude/rules/workflow.md  — development workflow
//!   - {apps_path}/{slug}/.claude/rules/docs.md      — docs.* MCP usage (obligatoire)
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

use crate::types::{AppStack, Application, Visibility};

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

        // Also write into src/ (where code-server opens projects — workspace root)
        let src_dir = app.src_dir();
        if src_dir.exists() {
            let src_claude_dir = src_dir.join(".claude");
            fs::create_dir_all(&src_claude_dir)?;
            log_write(&app.slug, &src_dir.join(".mcp.json"), &mcp_json)?;
            log_write(&app.slug, &src_claude_dir.join("settings.json"), &settings)?;

            // Clean up every legacy slash-command — tout est passé en skills.
            let commands_dir = src_claude_dir.join("commands");
            for legacy in OBSOLETE_SLASH_COMMANDS {
                remove_if_exists(&commands_dir.join(legacy), &app.slug);
            }
            // Retire le dossier commands/ s'il est devenu vide.
            if commands_dir.exists() {
                if let Ok(mut entries) = fs::read_dir(&commands_dir) {
                    if entries.next().is_none() {
                        let _ = fs::remove_dir(&commands_dir);
                    }
                }
            }

            let skills_root = src_claude_dir.join("skills");
            fs::create_dir_all(&skills_root)?;

            // Skill app-build (lazy-loaded) + script bundlé.
            let app_build_dir = skills_root.join("app-build");
            fs::create_dir_all(&app_build_dir)?;
            log_write(&app.slug, &app_build_dir.join("SKILL.md"), &render_app_build_skill(app))?;
            log_write(&app.slug, &app_build_dir.join("build.sh"), &render_app_build_script(app))?;

            // Skills auxiliaires (status, logs, db-info quand has_db).
            let produced: std::collections::HashSet<&'static str> = render_extra_skills(app)
                .iter()
                .map(|(name, _)| *name)
                .collect();
            for (name, content) in render_extra_skills(app) {
                let skill_dir = skills_root.join(name);
                fs::create_dir_all(&skill_dir)?;
                log_write(&app.slug, &skill_dir.join("SKILL.md"), &content)?;
            }
            // Nettoie les skills auxiliaires qui ne s'appliquent plus (ex: app-db-info
            // quand has_db est passé à false).
            for legacy_name in ALL_EXTRA_SKILL_NAMES {
                if !produced.contains(legacy_name) {
                    let dir = skills_root.join(legacy_name);
                    if dir.exists() {
                        let _ = fs::remove_dir_all(&dir);
                    }
                }
            }

            // Cleanup obsolete app-build rule at src level (if ever written there).
            remove_if_exists(&src_claude_dir.join("rules").join("app-build.md"), &app.slug);
        }

        let mcp_tools = render_mcp_tools_md(app);
        log_write(&app.slug, &rules_dir.join("mcp-tools.md"), &mcp_tools)?;

        let workflow = self.render_workflow_md(app);
        log_write(&app.slug, &rules_dir.join("workflow.md"), &workflow)?;

        // Cleanup obsolete files at parent level:
        //   - ancien rules/app-build.md (remplacé par la skill)
        //   - skills/app-build/ écrite par erreur au mauvais niveau avant le fix workspace
        remove_if_exists(&rules_dir.join("app-build.md"), &app.slug);
        let stale_skill = claude_dir.join("skills").join("app-build").join("SKILL.md");
        remove_if_exists(&stale_skill, &app.slug);
        let stale_skill_dir = claude_dir.join("skills").join("app-build");
        if stale_skill_dir.exists() {
            let _ = fs::remove_dir_all(&stale_skill_dir);
        }

        let docs = render_docs_md(app);
        log_write(&app.slug, &rules_dir.join("docs.md"), &docs)?;

        if matches!(app.stack, AppStack::Flutter) {
            let store_pub = render_store_publishing_md(app);
            log_write(&app.slug, &rules_dir.join("store-publishing.md"), &store_pub)?;
        } else {
            let legacy = rules_dir.join("store-publishing.md");
            if legacy.exists() {
                let _ = fs::remove_file(&legacy);
            }
        }

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
             - **Run:** `{run_command}`\n\
             - **Health path:** `{health_path}`\n\
             \n\
             > **Build** : utilise la skill `app-build`. Ne lance jamais `{build_cmd}` manuellement — ça se fait sur CloudMaster.\n\
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
             `db.query`, `docs.get`, `docs.list`, `docs.search`) are auto-approved. \
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
             `app.status`, `app.logs`, `db.tables`, `db.schema`, `db.query`, \
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
             control it — **never** lancer le binaire à la main (`nohup`, `tmux`, \
             `./bin/xxx &`, `cargo run`, `systemctl`, `kill`).\n\
             \n\
             ## Interdits (et pourquoi)\n\
             - **Lancer le binaire à la main** : le superviseur vérifie que le port \
             `{port}` est libre avant de spawner. Un process manuel sur ce port bloque \
             `app.control start` avec `port not free` — l'app semble morte pour \
             l'orchestrateur alors qu'elle tourne. Pour tester un binaire : \
             `app.control restart` + `app.logs`, jamais `nohup`.\n\
             - **`kill -9` du process supervisé** : le superviseur le relance avec \
             backoff. Utilise `app.control stop`.\n\
             - **Binder un autre service sur `{port}`** : même symptôme que le nohup.\n\
             \n\
             ## Debug d'un démarrage qui échoue\n\
             1. `app.status` → state (`crashed`, `stopped`, `running`) + restart_count.\n\
             2. `app.logs` → lignes orchestrateur : `port not free`, `spawn failed`, \
             `process exited code=...`.\n\
             3. Vérifier que `{run_command}` existe et est exécutable dans `{src_dir}`.\n\
             4. Si tout semble OK mais rien ne démarre → `ss -lntp | grep {port}` via \
             `app.exec` pour voir qui squatte le port.\n\
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
            port = app.port,
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
         ## Build\n\
         Pour builder cette app, utilise la skill **app-build** (lazy-loaded). Elle appelle l'endpoint HTTP bloquant via Bash.\n\
         \n\
",
        name = app.name,
        slug = app.slug,
    )
}

fn render_docs_md(app: &Application) -> String {
    format!(
        "# Documentation — {name} (OBLIGATOIRE)\n\
         \n\
         Chaque application HomeRoute possède une documentation centralisée accessible \
         via les tools MCP `docs.*`. Tu **DOIS** la lire et la tenir à jour — c'est \
         ce qui permet aux futures sessions (et aux autres agents) de comprendre l'app \
         sans relire tout le code.\n\
         \n\
         ## Règles obligatoires\n\
         \n\
         ### Avant de modifier l'app\n\
         - **TOUJOURS** appeler `docs.get` avec `app_id = \"{slug}\"` avant toute \
         modification significative.\n\
         - Lire au minimum les sections pertinentes (`structure`, `features`, `backend`) \
         pour éviter les incohérences avec les décisions passées.\n\
         \n\
         ### Après modification de l'app\n\
         - **TOUJOURS** mettre à jour la doc via `docs.update` quand :\n\
         \n\
         | Changement | Section à mettre à jour |\n\
         |---|---|\n\
         | Nouvelle feature utilisateur | `features` |\n\
         | Structure / architecture modifiée | `structure` |\n\
         | API, routes, logique backend | `backend` |\n\
         | Nom, stack, description, logo | `meta` (JSON) |\n\
         | Décision notable, TODO, remarque | `notes` |\n\
         \n\
         ### Vérification de complétude\n\
         - Après mise à jour, appeler `docs.completeness` avec `app_id = \"{slug}\"` \
         pour repérer les sections vides.\n\
         - Si des sections sont vides **et** que l'information est disponible → les remplir.\n\
         \n\
         ## Style de documentation\n\
         \n\
         - Descriptions **orientées utilisateur**, pas techniques.\n\
         - Les features décrivent **ce que l'utilisateur peut faire**, pas l'implémentation.\n\
         \n\
         ✅ Bon : « Page permettant aux utilisateurs de gérer leur profil et préférences »\n\
         \n\
         ❌ Mauvais : « Composant React avec useState qui fetch /api/users »\n\
         \n\
         ## Sections disponibles\n\
         \n\
         | Section | Format | Contenu |\n\
         |---|---|---|\n\
         | `meta` | JSON | `name`, `stack`, `description`, `logo` |\n\
         | `structure` | Markdown | Architecture, organisation du code |\n\
         | `features` | Markdown | Liste des fonctionnalités utilisateur |\n\
         | `backend` | Markdown | API, routes, logique serveur |\n\
         | `notes` | Markdown | Notes générales, décisions, TODOs |\n\
         \n\
         ## Tools MCP (rappel)\n\
         \n\
         | Tool | Usage |\n\
         |---|---|\n\
         | `docs.list` | Lister toutes les apps documentées avec statut de complétude |\n\
         | `docs.get` | Lire la doc (toutes sections ou une seule) |\n\
         | `docs.update` | Mettre à jour une section (mutation) |\n\
         | `docs.search` | Recherche full-text dans toutes les docs |\n\
         | `docs.completeness` | Vérifier sections remplies vs vides |\n\
         \n\
         Sur cette app, passe `app_id = \"{slug}\"` à chaque appel.\n",
        name = app.name,
        slug = app.slug,
    )
}

/// Self-contained bash script that triggers the remote build and prints the
/// raw JSON response. Sourced from `SKILL.md` so the skill body stays focused
/// on *when* to build rather than *how*.
fn render_app_build_script(app: &Application) -> String {
    format!(
        "#!/usr/bin/env bash\n\
         # Déclenche un build distant de l'app `{slug}` sur CloudMaster.\n\
         # Géré par HomeRoute — ne pas éditer (régénéré à chaque AppUpdate).\n\
         set -euo pipefail\n\
         TIMEOUT_SECS=\"${{1:-1800}}\"\n\
         curl -sS --max-time \"$TIMEOUT_SECS\" -X POST \\\n\
           \"http://127.0.0.1:4000/api/apps/{slug}/build\" \\\n\
           -H 'content-type: application/json' \\\n\
           -d \"{{\\\"timeout_secs\\\":${{TIMEOUT_SECS}}}}\"\n",
        slug = app.slug,
    )
}

fn render_app_build_skill(app: &Application) -> String {
    use crate::types::AppStack;

    let stack_label = app.stack.display_name();
    let build_cmd = app
        .build_command
        .as_deref()
        .unwrap_or("(no build command configured)");

    let stack_section = match app.stack {
        AppStack::Axum => format!(
            "## Stack: Rust (Axum)\n\n\
             - Artefact rapatrié depuis CloudMaster : `target/release/{slug}` (ou `build_artefact` si défini).\n\
             - Build effectif côté CloudMaster : `{cmd}`.\n",
            slug = app.slug,
            cmd = build_cmd,
        ),
        AppStack::AxumVite => format!(
            "## Stack: Rust (Axum) + Vite\n\n\
             - Artefacts rapatriés : `target/release/{slug}` + `web/dist/` (ou `build_artefact` si défini).\n\
             - Build effectif côté CloudMaster : `{cmd}`.\n",
            slug = app.slug,
            cmd = build_cmd,
        ),
        AppStack::NextJs => format!(
            "## Stack: Next.js\n\n\
             - Artefacts rapatriés : `.next/`, `public/`, `package.json`, `package-lock.json`, `node_modules/` (ou `build_artefact` si défini).\n\
             - Build effectif côté CloudMaster : `{cmd}`.\n",
            cmd = build_cmd,
        ),
        AppStack::Flutter => format!(
            "## Stack: Flutter (mobile Android)\n\n\
             - Build sur CloudMaster (`flutter build apk --release`), publication via la règle `store-publishing.md`.\n\
             - Build effectif : `{cmd}`.\n",
            cmd = build_cmd,
        ),
    };

    format!(
        "---\n\
         name: app-build\n\
         description: Build l'application {slug} ({stack}) sur CloudMaster et rapatrie les artefacts. Utilise cette skill QUAND l'utilisateur demande de builder/compiler/rebuild cette app — ne lance JAMAIS le build manuellement.\n\
         allowed-tools: Bash(bash .claude/skills/app-build/build.sh*)\n\
         ---\n\
         \n\
         # Build de l'app `{slug}`\n\
         \n\
         Cette skill déclenche un build distant de l'app sur CloudMaster (10.0.0.10) et rapatrie les artefacts via rsync. \
         Tout passe par un endpoint HTTP bloquant — pas de build local.\n\
         \n\
         ## Commande\n\
         \n\
         ```bash\n\
         bash .claude/skills/app-build/build.sh\n\
         ```\n\
         \n\
         Le script prend un timeout optionnel en secondes (défaut 1800) :\n\
         \n\
         ```bash\n\
         bash .claude/skills/app-build/build.sh 3600\n\
         ```\n\
         \n\
         ## Retour\n\
         \n\
         Réponse JSON : `{{ ok, stages, summary, duration_ms }}`.\n\
         \n\
         - `ok: true` → build réussi, lire `summary` pour les détails.\n\
         - `ok: false` → lire le tableau `stages` pour identifier la phase fautive (ssh-probe, rsync-up, compile, rsync-back, restart).\n\
         \n\
         ## Erreur HTTP 409 — BUILD_BUSY\n\
         \n\
         Si l'endpoint répond **HTTP 409** (`BUILD_BUSY`), un autre build de cette app est déjà en cours \
         (probablement une autre conversation ou un autre code-server).\n\
         \n\
         **NE PAS RETRY automatiquement.** Tu DOIS :\n\
         \n\
         1. Informer l'utilisateur qu'un build est déjà en cours pour `{slug}`.\n\
         2. Attendre son feu vert explicite avant de relancer.\n\
         \n\
         Pourquoi : deux builds concurrents sur le même slug corrompraient les sources côté CloudMaster (rsync concurrent).\n\
         \n\
         ## Après build OK\n\
         \n\
         Redémarrer le process supervisé via le tool MCP `restart` (action `restart` sur l'app `{slug}`).\n\
         \n\
         ## Interdits\n\
         \n\
         - **JAMAIS** lancer `{cmd}` localement dans le studio — ça doit tourner sur CloudMaster.\n\
         - **JAMAIS** invoquer `cargo build`, `npm run build`, `pnpm build` à la main pour cette app.\n\
         \n\
         {stack_section}",
        slug = app.slug,
        stack = stack_label,
        cmd = build_cmd,
        stack_section = stack_section,
    )
}

/// Remove a stale file silently. Logs at info if the file existed.
fn remove_if_exists(path: &Path, slug: &str) {
    match fs::remove_file(path) {
        Ok(()) => {
            info!(slug = %slug, file = %path.display(), "obsolete context file removed");
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => {
            warn!(slug = %slug, file = %path.display(), error = %e, "failed to remove obsolete context file");
        }
    }
}

fn render_store_publishing_md(app: &Application) -> String {
    format!(
        "# Publication Store — Règles obligatoires\n\
         \n\
         Pour publier une nouvelle version de cette app Flutter dans le store mobile HomeRoute, \
         **toujours** utiliser le tool MCP `store.upload`. Jamais un `curl` manuel vers \
         `/api/store/apps/{slug}/releases`.\n\
         \n\
         ## Workflow\n\
         \n\
         1. Build APK sur CloudMaster :\n\
         ```\n\
         export PATH=/ssd_pool/flutter/bin:$PATH\n\
         flutter build apk --release\n\
         ```\n\
         2. Encoder en base64 :\n\
         ```\n\
         base64 -w0 build/app/outputs/flutter-apk/app-release.apk > /tmp/app.b64\n\
         ```\n\
         3. Appeler `store.upload` (tool MCP) avec :\n\
         - `slug` : `{slug}`\n\
         - `version` : la version à publier (ex: `1.2.3`)\n\
         - `apk_base64` : contenu du fichier `.b64`\n\
         \n\
         4. Vérifier via `store.get` (slug `{slug}`) que la nouvelle version apparaît avec \
         `sha256` et `size_bytes` renseignés.\n\
         \n\
         ## Arguments optionnels (store.upload)\n\
         \n\
         | Argument | Header HTTP | Usage |\n\
         |---|---|---|\n\
         | `app_name` | `X-App-Name` | **Requis au premier upload** de ce slug |\n\
         | `description` | `X-App-Description` | Description affichée dans le store |\n\
         | `category` | `X-App-Category` | Défaut : `other` |\n\
         | `changelog` | `X-Changelog` | Notes de version |\n\
         | `publisher_app_id` | `X-Publisher-App-Id` | Lien vers une app publisher |\n\
         \n\
         Package Android, SHA256, taille et icône sont extraits automatiquement de l'APK côté API.\n\
         \n\
         ## Limites\n\
         \n\
         - Payload max : 500 MB.\n\
         - Pas de streaming ni de upload par chunks.\n\
         - Suppression de release : via API REST directement, pas via MCP.\n",
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

/// Skills additionnelles (read-only) en plus de `app-build`. Chaque entrée est
/// (nom_skill, contenu_complet_avec_frontmatter). Le nom devient le dossier
/// `src/.claude/skills/<nom>/SKILL.md`.
fn render_extra_skills(app: &Application) -> Vec<(&'static str, String)> {
    let mut skills = vec![
        ("app-status", format!(
            "---\n\
             name: app-status\n\
             description: Affiche l'état courant du process de l'app {slug} (state, PID, port, uptime, restart count). Utilise-moi quand l'utilisateur demande le statut, l'état, si l'app tourne, son PID ou son uptime.\n\
             allowed-tools: \n\
             ---\n\
             \n\
             # Statut de l'app `{slug}`\n\
             \n\
             Appelle le tool MCP `status` et affiche le résultat de manière concise : \
             state, PID, port, uptime, restart count.\n",
            slug = app.slug,
        )),
        ("app-logs", format!(
            "---\n\
             name: app-logs\n\
             description: Récupère et analyse les logs récents de l'app {slug}. Utilise-moi quand l'utilisateur demande les logs, des erreurs récentes, pourquoi l'app crash, ou un diagnostic runtime.\n\
             allowed-tools: \n\
             ---\n\
             \n\
             # Logs de l'app `{slug}`\n\
             \n\
             Appelle le tool MCP `logs` (paramètres : `limit` optionnel, `level` optionnel). \
             Identifie toute erreur ou warning et suggère des actions si pertinent.\n",
            slug = app.slug,
        )),
    ];

    if app.has_db {
        skills.push(("app-db-info", format!(
            "---\n\
             name: app-db-info\n\
             description: Donne un résumé de la base SQLite de l'app {slug} (tables, colonnes, row counts). Utilise-moi quand l'utilisateur demande ce qu'il y a en base, le schéma, ou un aperçu des données.\n\
             allowed-tools: \n\
             ---\n\
             \n\
             # Résumé base `{slug}`\n\
             \n\
             1. Appelle `db_tables` pour lister toutes les tables.\n\
             2. Pour chaque table, appelle `db_schema` pour obtenir les colonnes et le row count.\n\
             3. Affiche un résumé concis : nom de la table, nombre de colonnes, nombre de lignes.\n",
            slug = app.slug,
        )));
    }

    skills
}

/// Slash-commands & fichiers legacy à nettoyer à chaque régénération.
/// Les builds sont désormais la skill `app-build` ; les raccourcis status/logs/db-info
/// sont devenus des skills — plus rien ne vit dans `src/.claude/commands/`.
const OBSOLETE_SLASH_COMMANDS: &[&str] = &[
    "build.md",
    "build-client.md",
    "build-server.md",
    "build-api.md",
    "build-apk.md",
    "publish-apk.md",
    "install.md",
    "deploy.md",
    "status.md",
    "logs.md",
    "db-info.md",
];

/// Noms de skills auxiliaires potentiellement obsolètes à nettoyer si la stack
/// de l'app change (ex: app passe de `has_db=true` à `false` → retirer app-db-info).
const ALL_EXTRA_SKILL_NAMES: &[&str] = &["app-status", "app-logs", "app-db-info"];

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

        // Create src/ so src_dir.exists() → true (code-server workspace root).
        fs::create_dir_all(tmp.join("trader/src")).unwrap();

        // Pré-créer un ancien rules/app-build.md bidon : doit être supprimé par generate_for_app.
        let stale_rules_dir = tmp.join("trader/.claude/rules");
        fs::create_dir_all(&stale_rules_dir).unwrap();
        let stale_path = stale_rules_dir.join("app-build.md");
        fs::write(&stale_path, "stale content").unwrap();
        assert!(stale_path.exists());

        ctx.generate_for_app(&trader, &all, Some(vec!["users".into(), "trades".into()]))
            .unwrap();

        // L'ancien fichier doit avoir été supprimé.
        assert!(!stale_path.exists(), "ancien rules/app-build.md doit être supprimé");

        let claude_md = fs::read_to_string(tmp.join("trader/CLAUDE.md")).unwrap();
        assert!(claude_md.contains("# Trader"));
        assert!(claude_md.contains("`trader`"));
        assert!(claude_md.contains("Vite+Rust"));
        assert!(claude_md.contains("`users`"));
        assert!(claude_md.contains("`trades`"));
        assert!(claude_md.contains("Wallet"));
        assert!(claude_md.contains("`API_KEY`"));

        let settings = fs::read_to_string(tmp.join("trader/.claude/settings.json")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&settings).unwrap();
        assert_eq!(
            parsed["mcpServers"]["homeroute"]["url"].as_str().unwrap(),
            "http://127.0.0.1:4001/mcp?project=trader"
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
        // Note: the app-build skill is written under `app.src_dir()/.claude/skills/`,
        // which uses a hardcoded /opt/homeroute/apps path from Application::src_dir().
        // We validate its content via render_app_build_skill directly below.
        let skill_content = render_app_build_skill(&trader);
        assert!(skill_content.contains("name: app-build"));
        assert!(skill_content.contains("allowed-tools: Bash(bash .claude/skills/app-build/build.sh"));
        assert!(skill_content.contains("bash .claude/skills/app-build/build.sh"));
        // Le curl est maintenant dans le script séparé.
        let script = render_app_build_script(&trader);
        assert!(script.contains("/api/apps/trader/build"));
        assert!(script.starts_with("#!/usr/bin/env bash"));
        assert!(tmp.join("trader/.claude/rules/docs.md").exists());

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

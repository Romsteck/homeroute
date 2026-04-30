//! IPC handlers for `App*` variants (hr-apps integration).
//!
//! Split out of `ipc_handler.rs` to keep that file manageable.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::scaffold;

use hr_apps::types::{AppStack, AppState, Application, SourcesLocation, Visibility, valid_slug};
use hr_apps::todos::{TodoStatus, TodosManager};
use hr_apps::{AppSupervisor, ContextGenerator, DbManager, ProcessStatus};
use hr_common::events::AppBuildEvent;
use hr_common::logging::LogStore;
use tokio::sync::broadcast;

fn detect_level(msg: &str) -> &'static str {
    let m = msg.to_ascii_lowercase();
    if m.contains("error") || m.contains("panic") || m.contains("fatal") {
        "error"
    } else if m.contains("warn") {
        "warn"
    } else if m.contains("debug") {
        "debug"
    } else {
        "info"
    }
}
use hr_ipc::EdgeClient;
use hr_ipc::types::{
    AppDbQueryResult, AppDbRelation, AppDbTableColumn, AppDbTableSchema,
    AppDbTablesData,
    AppExecResult, AppListData, AppLogEntry, AppLogsData, AppStatusData, ApplicationDto,
    IpcResponse,
};
use tracing::{error, info, warn};

/// Default remote build host (override via `HR_BUILD_HOST`).
pub const BUILD_HOST: &str = "romain@10.0.0.10";
/// Default SSH key path on Medion (override via `HR_BUILD_SSH_KEY`).
pub const SSH_KEY: &str = "/opt/homeroute/data/build/ssh/id_ed25519";
/// Cap stdout/stderr capture per pipeline stage to ~1 MB.
const OUTPUT_CAP_BYTES: usize = 1024 * 1024;

/// Context for App* handlers.
#[derive(Clone)]
pub struct AppsContext {
    pub supervisor: AppSupervisor,
    pub db_manager: DbManager,
    pub todos: TodosManager,
    pub context_generator: Arc<ContextGenerator>,
    pub edge: Arc<EdgeClient>,
    pub git: Arc<hr_git::GitService>,
    pub base_domain: String,
    pub log_store: Arc<LogStore>,
    /// Per-slug locks to serialise concurrent `build()` invocations.
    pub build_locks:
        Arc<tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
    /// Broadcast channel for build progress events.
    pub app_build_tx: broadcast::Sender<AppBuildEvent>,
}

impl AppsContext {
    pub async fn list(&self) -> IpcResponse {
        let apps = self.supervisor.registry.list().await;
        info!(count = apps.len(), "AppList");
        let dtos: Vec<ApplicationDto> = apps.iter().map(app_to_dto).collect();
        IpcResponse::ok_data(AppListData { apps: dtos })
    }

    pub async fn get(&self, slug: &str) -> IpcResponse {
        if !valid_slug(slug) {
            return IpcResponse::err("invalid slug");
        }
        match self.supervisor.registry.get(slug).await {
            Some(app) => {
                info!(slug = %slug, "AppGet");
                IpcResponse::ok_data(app_to_dto(&app))
            }
            None => IpcResponse::err(format!("app not found: {slug}")),
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[tracing::instrument(skip(self), fields(slug = %slug))]
    pub async fn create(
        &self,
        slug: String,
        name: String,
        stack: String,
        has_db: bool,
        visibility: String,
        run_command: Option<String>,
        build_command: Option<String>,
        health_path: Option<String>,
        build_artefact: Option<String>,
    ) -> IpcResponse {
        let start = Instant::now();
        if !valid_slug(&slug) {
            return IpcResponse::err("invalid slug");
        }
        if self.supervisor.registry.get(&slug).await.is_some() {
            return IpcResponse::err(format!("app already exists: {slug}"));
        }

        let stack_enum = match parse_stack(&stack) {
            Some(s) => s,
            None => return IpcResponse::err(format!("invalid stack: {stack}")),
        };
        let visibility_enum = match parse_visibility(&visibility) {
            Some(v) => v,
            None => return IpcResponse::err(format!("invalid visibility: {visibility}")),
        };

        // Assign port BEFORE creating the Application so it is persisted.
        let port = match self.supervisor.port_registry.assign(&slug).await {
            Ok(p) => p,
            Err(e) => {
                error!(slug = %slug, error = %e, "AppCreate: port assignment failed");
                return IpcResponse::err(format!("port assignment failed: {e}"));
            }
        };

        let mut app = Application::new(slug.clone(), name, stack_enum);
        app.has_db = has_db;
        app.visibility = visibility_enum;
        app.port = port;
        app.domain = format!("{}.{}", slug, self.base_domain);
        if let Some(cmd) = run_command {
            app.run_command = cmd;
        }
        app.build_command = build_command;
        app.build_artefact = build_artefact;
        if let Some(hp) = health_path {
            app.health_path = hp;
        }
        // SourcesLocation::default() == CloudMaster (Phase 7) — `Application::new`
        // l'a déjà appliqué. On l'expose explicitement pour que la suite du
        // handler raisonne dessus.
        let sources_on = app.sources_on;
        info!(slug = %slug, sources_on = ?sources_on, "AppCreate: scaffolding new app");

        // Toujours créer app_dir local (Medion) — il porte db.sqlite et .env runtime.
        let app_dir = app.app_dir();
        if let Err(e) = tokio::fs::create_dir_all(&app_dir).await {
            error!(slug = %slug, error = %e, "AppCreate: create app_dir failed");
            self.supervisor.port_registry.release(&slug).await.ok();
            return IpcResponse::err(format!("create app dir failed: {e}"));
        }

        // Scaffold + contexte selon `sources_on`.
        match sources_on {
            SourcesLocation::Medion => {
                // Layout legacy : src/ vit sur Medion.
                if let Err(e) = tokio::fs::create_dir_all(&app.src_dir()).await {
                    warn!(slug = %slug, error = %e, "AppCreate: create src_dir failed");
                }
                if let Err(e) = scaffold::scaffold_stack_template(&app).await {
                    warn!(slug = %slug, error = %e, "AppCreate: scaffold template failed (non-fatal)");
                }
            }
            SourcesLocation::CloudMaster => {
                // Layout cible (Phase 7) : src/ vit sur CloudMaster. Génération
                // dans un tmpdir local puis rsync UP, suivi d'un chown romain.
                if let Err(e) = scaffold_on_cloudmaster(&app, &self.context_generator,
                                                         &self.supervisor.registry.list().await).await
                {
                    error!(slug = %slug, error = %e, "AppCreate: cloudmaster scaffold failed");
                    self.supervisor.port_registry.release(&slug).await.ok();
                    return IpcResponse::err(format!("cloudmaster scaffold failed: {e}"));
                }
            }
        }

        // Default run_command si non fourni.
        if app.run_command.trim().is_empty() {
            app.run_command = scaffold::default_run_command(&app);
            info!(slug = %slug, run_command = %app.run_command, "AppCreate: applied default run_command");
        }
        if has_db {
            let _ = tokio::fs::File::create(app.db_path()).await;
        }

        // Persist app (sources_on figé dans apps.json à ce moment).
        if let Err(e) = self.supervisor.registry.upsert(app.clone()).await {
            self.supervisor.port_registry.release(&slug).await.ok();
            error!(slug = %slug, error = %e, "AppCreate: registry upsert failed");
            return IpcResponse::err(format!("registry upsert failed: {e}"));
        }

        // hr-git bare repo (best-effort).
        if let Err(e) = self.git.create_repo(&slug).await {
            warn!(slug = %slug, error = %e, "AppCreate: git create_repo failed (non-fatal)");
        }

        // hr-edge route (best-effort).
        let auth_required = matches!(app.visibility, Visibility::Private);
        if let Err(e) = self
            .edge
            .set_app_route(
                app.domain.clone(),
                slug.clone(),
                "local".to_string(),
                "127.0.0.1".to_string(),
                port,
                auth_required,
                vec![],
                false,
            )
            .await
        {
            warn!(slug = %slug, domain = %app.domain, error = %e, "AppCreate: edge set_app_route failed (non-fatal)");
        }

        // Regen context. En CloudMaster c'est déjà fait dans scaffold_on_cloudmaster
        // (pour rsync UP en bloc) ; on ne l'appelle ici que pour Medion.
        let all = self.supervisor.registry.list().await;
        if matches!(sources_on, SourcesLocation::Medion) {
            let db_tables = if app.has_db {
                self.db_manager.list_tables(&slug).await.ok()
            } else {
                None
            };
            if let Err(e) = self
                .context_generator
                .generate_for_app(&app, &all, db_tables)
            {
                warn!(slug = %slug, error = %e, "AppCreate: context generation failed (non-fatal)");
            }
        }
        if let Err(e) = self.context_generator.generate_root(&all) {
            warn!(error = %e, "AppCreate: root context generation failed (non-fatal)");
        }

        info!(slug = %slug, port, sources_on = ?sources_on, duration_ms = start.elapsed().as_millis() as u64, "AppCreate ok");
        IpcResponse::ok_data(app_to_dto(&app))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update(
        &self,
        slug: String,
        name: Option<String>,
        visibility: Option<String>,
        run_command: Option<String>,
        build_command: Option<String>,
        health_path: Option<String>,
        env_vars: Option<BTreeMap<String, String>>,
        has_db: Option<bool>,
        build_artefact: Option<String>,
    ) -> IpcResponse {
        if !valid_slug(&slug) {
            return IpcResponse::err("invalid slug");
        }
        let mut app = match self.supervisor.registry.get(&slug).await {
            Some(a) => a,
            None => return IpcResponse::err(format!("app not found: {slug}")),
        };

        if let Some(n) = name {
            app.name = n;
        }
        if let Some(v) = visibility {
            match parse_visibility(&v) {
                Some(vv) => app.visibility = vv,
                None => return IpcResponse::err(format!("invalid visibility: {v}")),
            }
        }
        if let Some(rc) = run_command {
            app.run_command = rc;
        }
        if build_command.is_some() {
            app.build_command = build_command;
        }
        if build_artefact.is_some() {
            app.build_artefact = build_artefact;
        }
        if let Some(hp) = health_path {
            app.health_path = hp;
        }
        if let Some(ev) = env_vars {
            app.env_vars = ev;
        }
        if let Some(new_has_db) = has_db {
            if new_has_db && !app.has_db {
                // Enable: create the DB file if missing so the engine can initialize metadata.
                if !app.db_path().exists() {
                    if let Err(e) = tokio::fs::File::create(app.db_path()).await {
                        return IpcResponse::err(format!("failed to create db file: {e}"));
                    }
                }
                info!(slug = %slug, "has_db enabled");
            } else if !new_has_db && app.has_db {
                // Disable: keep the DB file on disk; only flip the flag.
                info!(slug = %slug, "has_db disabled (db file preserved on disk)");
            }
            app.has_db = new_has_db;
        }

        if let Err(e) = self.supervisor.registry.upsert(app.clone()).await {
            error!(slug = %slug, error = %e, "AppUpdate: registry upsert failed");
            return IpcResponse::err(format!("registry upsert failed: {e}"));
        }

        // Push updated edge route if visibility changed
        let auth_required = matches!(app.visibility, Visibility::Private);
        if let Err(e) = self
            .edge
            .set_app_route(
                app.domain.clone(),
                slug.clone(),
                "local".to_string(),
                "127.0.0.1".to_string(),
                app.port,
                auth_required,
                vec![],
                false,
            )
            .await
        {
            warn!(slug = %slug, error = %e, "AppUpdate: edge set_app_route failed (non-fatal)");
        }

        // Regenerate context
        let all = self.supervisor.registry.list().await;
        let db_tables = if app.has_db {
            self.db_manager.list_tables(&slug).await.ok()
        } else {
            None
        };
        if let Err(e) = self
            .context_generator
            .generate_for_app(&app, &all, db_tables)
        {
            warn!(slug = %slug, error = %e, "AppUpdate: context regeneration failed");
        }

        info!(slug = %slug, "AppUpdate ok");
        IpcResponse::ok_data(app_to_dto(&app))
    }

    #[tracing::instrument(skip(self), fields(slug = %slug, keep_data))]
    pub async fn delete(&self, slug: String, keep_data: bool) -> IpcResponse {
        if !valid_slug(&slug) {
            return IpcResponse::err("invalid slug");
        }
        let app = match self.supervisor.registry.get(&slug).await {
            Some(a) => a,
            None => return IpcResponse::err(format!("app not found: {slug}")),
        };

        // 1. Stop process
        if let Err(e) = self.supervisor.stop(&slug).await {
            warn!(slug = %slug, error = %e, "AppDelete: stop failed (continuing)");
        }
        // 2. Remove edge route
        if let Err(e) = self.edge.remove_app_route(&app.domain).await {
            warn!(slug = %slug, domain = %app.domain, error = %e, "AppDelete: edge remove_app_route failed");
        }
        // 3. Remove from registry
        if let Err(e) = self.supervisor.registry.remove(&slug).await {
            error!(slug = %slug, error = %e, "AppDelete: registry remove failed");
            return IpcResponse::err(format!("registry remove failed: {e}"));
        }
        // 4. Release port
        if let Err(e) = self.supervisor.port_registry.release(&slug).await {
            warn!(slug = %slug, error = %e, "AppDelete: port release failed");
        }
        if !keep_data {
            // 5. Cleanup CloudMaster src/ si applicable (avant Medion pour
            // garder l'ordre symétrique de scaffold).
            if matches!(app.sources_on, SourcesLocation::CloudMaster) {
                if let Err(e) = cleanup_cloudmaster_src(&slug).await {
                    warn!(slug = %slug, error = %e, "AppDelete: cloudmaster cleanup failed (non-fatal)");
                }
            }
            // 6. Cleanup Medion app_dir
            let dir: PathBuf = PathBuf::from(format!("/opt/homeroute/apps/{}", slug));
            if let Err(e) = tokio::fs::remove_dir_all(&dir).await {
                warn!(slug = %slug, dir = %dir.display(), error = %e, "AppDelete: rm -rf failed");
            }
        } else {
            info!(slug = %slug, "AppDelete: keep_data=true, sources préservées (Medion+CloudMaster)");
        }

        // 7. Regenerate root context
        let all = self.supervisor.registry.list().await;
        if let Err(e) = self.context_generator.generate_root(&all) {
            warn!(error = %e, "AppDelete: root context regeneration failed");
        }

        info!(slug = %slug, keep_data, sources_on = ?app.sources_on, "AppDelete ok");
        IpcResponse::ok_data(serde_json::json!({ "ok": true }))
    }

    pub async fn control(&self, slug: String, action: String) -> IpcResponse {
        if !valid_slug(&slug) {
            return IpcResponse::err("invalid slug");
        }
        let start = Instant::now();
        let res = match action.as_str() {
            "start" => self.supervisor.start(&slug).await,
            "stop" => self.supervisor.stop(&slug).await,
            "restart" => self.supervisor.restart(&slug).await,
            other => return IpcResponse::err(format!("invalid action: {other}")),
        };

        match res {
            Ok(()) => {
                info!(
                    slug = %slug,
                    action = %action,
                    duration_ms = start.elapsed().as_millis() as u64,
                    "AppControl ok"
                );
                IpcResponse::ok_data(serde_json::json!({ "ok": true }))
            }
            Err(e) => {
                error!(slug = %slug, action = %action, error = %e, "AppControl failed");
                IpcResponse::err(format!("{action} failed: {e}"))
            }
        }
    }

    pub async fn status(&self, slug: &str) -> IpcResponse {
        if !valid_slug(slug) {
            return IpcResponse::err("invalid slug");
        }
        match self.supervisor.status(slug).await {
            Some(s) => IpcResponse::ok_data(process_status_to_dto(slug, &s)),
            None => {
                // Return a Stopped placeholder so callers don't 404 on never-started apps.
                let port = self.supervisor.port_registry.get(slug).await.unwrap_or(0);
                IpcResponse::ok_data(AppStatusData {
                    slug: slug.to_string(),
                    pid: None,
                    state: "stopped".to_string(),
                    port,
                    uptime_secs: 0,
                    restart_count: 0,
                })
            }
        }
    }

    pub async fn logs(
        &self,
        slug: String,
        limit: Option<usize>,
        level: Option<String>,
    ) -> IpcResponse {
        if !valid_slug(&slug) {
            return IpcResponse::err("invalid slug");
        }
        let n = limit.unwrap_or(200).min(5000);
        let unit = format!("hr-app-{slug}.service");
        let output = tokio::process::Command::new("journalctl")
            .args([
                "-u",
                &unit,
                "-n",
                &n.to_string(),
                "--no-pager",
                "--output=short-iso",
            ])
            .output()
            .await;
        match output {
            Ok(out) if out.status.success() => {
                let text = String::from_utf8_lossy(&out.stdout);
                let level_filter = level.as_deref();
                let mut logs: Vec<AppLogEntry> = Vec::new();
                for line in text.lines() {
                    if line.starts_with("--") || line.is_empty() {
                        continue;
                    }
                    // Format: "2024-01-02T10:11:12+0000 host unit[pid]: message"
                    let (timestamp, rest) = match line.split_once(' ') {
                        Some(p) => p,
                        None => continue,
                    };
                    let msg = match rest.find("]: ") {
                        Some(i) => rest[i + 3..].to_string(),
                        None => match rest.find(": ") {
                            Some(i) => rest[i + 2..].to_string(),
                            None => rest.to_string(),
                        },
                    };
                    let lvl = detect_level(&msg);
                    if let Some(f) = level_filter {
                        if !lvl.eq_ignore_ascii_case(f) {
                            continue;
                        }
                    }
                    logs.push(AppLogEntry {
                        timestamp: timestamp.to_string(),
                        level: lvl.to_string(),
                        message: msg,
                        data: None,
                    });
                }
                info!(slug = %slug, count = logs.len(), "AppLogs queried (journald)");
                IpcResponse::ok_data(AppLogsData { slug, logs })
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                warn!(slug = %slug, status = %out.status, stderr = %stderr, "journalctl failed");
                IpcResponse::ok_data(AppLogsData { slug, logs: vec![] })
            }
            Err(e) => {
                error!(slug = %slug, error = %e, "journalctl spawn failed");
                IpcResponse::err(format!("log query failed: {e}"))
            }
        }
    }

    pub async fn exec(
        &self,
        slug: String,
        command: String,
        timeout_secs: Option<u64>,
    ) -> IpcResponse {
        if !valid_slug(&slug) {
            return IpcResponse::err("invalid slug");
        }
        let app = match self.supervisor.registry.get(&slug).await {
            Some(a) => a,
            None => return IpcResponse::err(format!("app not found: {slug}")),
        };
        let cwd = app.src_dir();
        let timeout = std::time::Duration::from_secs(timeout_secs.unwrap_or(60).max(1));
        let start = Instant::now();

        let child = tokio::process::Command::new("/bin/bash")
            .arg("-c")
            .arg(&command)
            .current_dir(&cwd)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn();

        let child = match child {
            Ok(c) => c,
            Err(e) => {
                error!(slug = %slug, error = %e, "AppExec spawn failed");
                return IpcResponse::err(format!("spawn: {e}"));
            }
        };

        let wait_res = tokio::time::timeout(timeout, child.wait_with_output()).await;
        let out = match wait_res {
            Ok(Ok(o)) => o,
            Ok(Err(e)) => {
                return IpcResponse::err(format!("wait: {e}"));
            }
            Err(_) => {
                return IpcResponse::err(format!("timeout after {}s", timeout.as_secs()));
            }
        };

        let result = AppExecResult {
            stdout: String::from_utf8_lossy(&out.stdout).to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).to_string(),
            exit_code: out.status.code().unwrap_or(-1),
            duration_ms: start.elapsed().as_millis() as u64,
        };
        info!(
            slug = %slug,
            exit_code = result.exit_code,
            duration_ms = result.duration_ms,
            "AppExec ok"
        );
        IpcResponse::ok_data(result)
    }

    pub async fn regenerate_context(&self, slug: String) -> IpcResponse {
        if !valid_slug(&slug) {
            return IpcResponse::err("invalid slug");
        }
        let app = match self.supervisor.registry.get(&slug).await {
            Some(a) => a,
            None => return IpcResponse::err(format!("app not found: {slug}")),
        };
        let all = self.supervisor.registry.list().await;
        let db_tables = if app.has_db {
            self.db_manager.list_tables(&slug).await.ok()
        } else {
            None
        };

        match app.sources_on {
            SourcesLocation::Medion => {
                if let Err(e) = self
                    .context_generator
                    .generate_for_app(&app, &all, db_tables)
                {
                    error!(slug = %slug, error = %e, "AppRegenerateContext failed (medion)");
                    return IpcResponse::err(format!("generate_for_app: {e}"));
                }
            }
            SourcesLocation::CloudMaster => {
                if let Err(e) =
                    regen_context_on_cloudmaster(&app, &self.context_generator, &all, db_tables)
                        .await
                {
                    error!(slug = %slug, error = %e, "AppRegenerateContext failed (cloudmaster)");
                    return IpcResponse::err(format!("regen on cloudmaster: {e}"));
                }
            }
        }

        if let Err(e) = self.context_generator.generate_root(&all) {
            warn!(error = %e, "AppRegenerateContext root failed");
        }
        info!(slug = %slug, sources_on = ?app.sources_on, "AppRegenerateContext ok");
        IpcResponse::ok_data(serde_json::json!({ "ok": true }))
    }

    // ── App DB ─────────────────────────────────────────────────

    pub async fn db_list_tables(&self, slug: String) -> IpcResponse {
        if !valid_slug(&slug) {
            return IpcResponse::err("invalid slug");
        }
        match self.db_manager.list_tables(&slug).await {
            Ok(tables) => {
                info!(slug = %slug, count = tables.len(), "AppDbListTables ok");
                IpcResponse::ok_data(AppDbTablesData { tables })
            }
            Err(e) => {
                error!(slug = %slug, error = %e, "AppDbListTables failed");
                IpcResponse::err(format!("list_tables: {e}"))
            }
        }
    }

    pub async fn db_describe_table(&self, slug: String, table: String) -> IpcResponse {
        if !valid_slug(&slug) {
            return IpcResponse::err("invalid slug");
        }
        match self.db_manager.describe_table(&slug, &table).await {
            Ok(schema) => {
                let dto = AppDbTableSchema {
                    name: schema.name,
                    columns: schema
                        .columns
                        .into_iter()
                        .map(|c| AppDbTableColumn {
                            name: c.name,
                            field_type: format!("{:?}", c.field_type),
                            required: c.required,
                            unique: c.unique,
                            choices: c.choices,
                            formula_expression: c.formula_expression,
                        })
                        .collect(),
                    relations: schema
                        .relations
                        .into_iter()
                        .map(|r| AppDbRelation {
                            from_column: r.from_column,
                            to_table: r.to_table,
                            to_column: r.to_column,
                            display_column: r.display_column,
                        })
                        .collect(),
                    row_count: schema.row_count,
                };
                IpcResponse::ok_data(dto)
            }
            Err(e) => IpcResponse::err(format!("describe_table: {e}")),
        }
    }

    pub async fn db_query(
        &self,
        slug: String,
        sql: String,
        params: Vec<serde_json::Value>,
    ) -> IpcResponse {
        if !valid_slug(&slug) {
            return IpcResponse::err("invalid slug");
        }
        match self.db_manager.query(&slug, &sql, params).await {
            Ok(q) => {
                info!(slug = %slug, rows = q.total, "AppDbQuery ok");
                IpcResponse::ok_data(AppDbQueryResult {
                    columns: q.columns,
                    rows: q.rows,
                    total: q.total,
                })
            }
            Err(e) => {
                warn!(slug = %slug, error = %e, "AppDbQuery failed");
                IpcResponse::err(format!("query: {e}"))
            }
        }
    }

    pub async fn db_execute(
        &self,
        slug: String,
        sql: String,
        params: Vec<serde_json::Value>,
    ) -> IpcResponse {
        if !valid_slug(&slug) {
            return IpcResponse::err("invalid slug");
        }
        info!(slug = %slug, sql_preview = %sql.chars().take(80).collect::<String>(), "AppDbExecute");
        match self.db_manager.execute(&slug, &sql, params).await {
            Ok(rows_affected) => {
                info!(slug = %slug, rows_affected, "AppDbExecute ok");
                IpcResponse::ok_data(serde_json::json!({ "rows_affected": rows_affected }))
            }
            Err(e) => {
                warn!(slug = %slug, error = %e, "AppDbExecute failed");
                IpcResponse::err(format!("execute: {e}"))
            }
        }
    }

    pub async fn db_query_rows(
        &self,
        slug: String,
        table: String,
        filters_json: Vec<serde_json::Value>,
        limit: Option<u64>,
        offset: Option<u64>,
        order_by: Option<String>,
        order_desc: Option<bool>,
        expand: Vec<String>,
    ) -> IpcResponse {
        if !valid_slug(&slug) {
            return IpcResponse::err("invalid slug");
        }

        // Parse filters from JSON
        let filters: Vec<hr_apps::Filter> = filters_json
            .iter()
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect();

        let pagination = hr_apps::Pagination {
            limit: limit.unwrap_or(100),
            offset: offset.unwrap_or(0),
            order_by,
            order_desc: order_desc.unwrap_or(false),
        };

        match self
            .db_manager
            .select_rows_expanded(&slug, &table, &filters, &pagination, &expand)
            .await
        {
            Ok(q) => {
                info!(slug = %slug, table = %table, rows = q.total, expand = ?expand, "AppDbQueryRows ok");
                IpcResponse::ok_data(AppDbQueryResult {
                    columns: q.columns,
                    rows: q.rows,
                    total: q.total,
                })
            }
            Err(e) => {
                warn!(slug = %slug, table = %table, error = %e, "AppDbQueryRows failed");
                IpcResponse::err(format!("query_rows: {e}"))
            }
        }
    }

    pub async fn db_sync_schema(&self, slug: String) -> IpcResponse {
        if !valid_slug(&slug) {
            return IpcResponse::err("invalid slug");
        }
        match self.db_manager.sync_schema(&slug).await {
            Ok(r) => {
                info!(slug = %slug, tables = r.tables_added.len(), columns = r.columns_added.len(), relations = r.relations_added, "Schema sync done");
                IpcResponse::ok_data(r)
            }
            Err(e) => IpcResponse::err(format!("sync_schema: {e}")),
        }
    }

    pub async fn db_get_schema(&self, slug: String) -> IpcResponse {
        if !valid_slug(&slug) {
            return IpcResponse::err("invalid slug");
        }
        match self.db_manager.get_schema(&slug).await {
            Ok(schema) => IpcResponse::ok_data(schema),
            Err(e) => IpcResponse::err(format!("get_schema: {e}")),
        }
    }

    pub async fn db_create_table(&self, slug: String, definition: serde_json::Value) -> IpcResponse {
        if !valid_slug(&slug) {
            return IpcResponse::err("invalid slug");
        }
        // Fill in defaults so callers only need to supply `name` and `columns`.
        // `slug` defaults to `name`, and timestamps to `now` — all easily overridable.
        let mut def_value = definition;
        if let serde_json::Value::Object(ref mut map) = def_value {
            let now = chrono::Utc::now().to_rfc3339();
            if !map.contains_key("slug") {
                if let Some(name) = map.get("name").and_then(|v| v.as_str()) {
                    map.insert("slug".to_string(), serde_json::Value::String(name.to_string()));
                }
            }
            map.entry("created_at".to_string())
                .or_insert_with(|| serde_json::Value::String(now.clone()));
            map.entry("updated_at".to_string())
                .or_insert_with(|| serde_json::Value::String(now));
        }
        let def: hr_apps::TableDefinition = match serde_json::from_value(def_value) {
            Ok(d) => d,
            Err(e) => return IpcResponse::err(format!("invalid table definition: {e}")),
        };
        info!(slug = %slug, table = %def.name, "Creating table");
        match self.db_manager.create_table(&slug, def).await {
            Ok(version) => IpcResponse::ok_data(serde_json::json!({ "version": version })),
            Err(e) => IpcResponse::err(format!("create_table: {e}")),
        }
    }

    pub async fn db_drop_table(&self, slug: String, table: String) -> IpcResponse {
        if !valid_slug(&slug) {
            return IpcResponse::err("invalid slug");
        }
        info!(slug = %slug, table = %table, "Dropping table");
        match self.db_manager.drop_table(&slug, &table).await {
            Ok(version) => IpcResponse::ok_data(serde_json::json!({ "version": version })),
            Err(e) => IpcResponse::err(format!("drop_table: {e}")),
        }
    }

    pub async fn db_add_column(&self, slug: String, table: String, column: serde_json::Value) -> IpcResponse {
        if !valid_slug(&slug) {
            return IpcResponse::err("invalid slug");
        }
        let col: hr_apps::ColumnDefinition = match serde_json::from_value(column) {
            Ok(c) => c,
            Err(e) => return IpcResponse::err(format!("invalid column definition: {e}")),
        };
        info!(slug = %slug, table = %table, column = %col.name, "Adding column");
        match self.db_manager.add_column(&slug, &table, col).await {
            Ok(version) => IpcResponse::ok_data(serde_json::json!({ "version": version })),
            Err(e) => IpcResponse::err(format!("add_column: {e}")),
        }
    }

    pub async fn db_remove_column(&self, slug: String, table: String, column: String) -> IpcResponse {
        if !valid_slug(&slug) {
            return IpcResponse::err("invalid slug");
        }
        info!(slug = %slug, table = %table, column = %column, "Removing column");
        match self.db_manager.remove_column(&slug, &table, &column).await {
            Ok(version) => IpcResponse::ok_data(serde_json::json!({ "version": version })),
            Err(e) => IpcResponse::err(format!("remove_column: {e}")),
        }
    }

    pub async fn db_create_relation(&self, slug: String, relation: serde_json::Value) -> IpcResponse {
        if !valid_slug(&slug) {
            return IpcResponse::err("invalid slug");
        }
        let rel: hr_apps::RelationDefinition = match serde_json::from_value(relation) {
            Ok(r) => r,
            Err(e) => return IpcResponse::err(format!("invalid relation definition: {e}")),
        };
        info!(slug = %slug, from = %rel.from_table, to = %rel.to_table, "Creating relation");
        match self.db_manager.create_relation(&slug, rel).await {
            Ok(version) => IpcResponse::ok_data(serde_json::json!({ "version": version })),
            Err(e) => IpcResponse::err(format!("create_relation: {e}")),
        }
    }

    // ── Todos (per-app JSON store, live via app_todos event) ─────

    pub async fn todos_list(&self, slug: String, status: Option<String>) -> IpcResponse {
        if !valid_slug(&slug) {
            return IpcResponse::err("invalid slug");
        }
        let filter = match status.as_deref() {
            None => None,
            Some(s) => match TodoStatus::parse(s) {
                Ok(v) => Some(v),
                Err(e) => return IpcResponse::err(e.to_string()),
            },
        };
        match self.todos.list(&slug, filter).await {
            Ok(todos) => {
                info!(slug = %slug, count = todos.len(), "AppTodosList ok");
                IpcResponse::ok_data(serde_json::json!({ "todos": todos }))
            }
            Err(e) => {
                error!(slug = %slug, error = %e, "AppTodosList failed");
                IpcResponse::err(format!("todos_list: {e}"))
            }
        }
    }

    pub async fn todos_create(
        &self,
        slug: String,
        name: String,
        description: Option<String>,
    ) -> IpcResponse {
        if !valid_slug(&slug) {
            return IpcResponse::err("invalid slug");
        }
        match self.todos.create(&slug, name, description).await {
            Ok(todo) => {
                info!(slug = %slug, id = %todo.id, "AppTodosCreate ok");
                IpcResponse::ok_data(serde_json::json!({ "todo": todo }))
            }
            Err(e) => {
                error!(slug = %slug, error = %e, "AppTodosCreate failed");
                IpcResponse::err(format!("todos_create: {e}"))
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn todos_update(
        &self,
        slug: String,
        id: String,
        name: Option<String>,
        description: Option<String>,
        status: Option<String>,
        status_reason: Option<String>,
    ) -> IpcResponse {
        if !valid_slug(&slug) {
            return IpcResponse::err("invalid slug");
        }
        let status_enum = match status.as_deref() {
            None => None,
            Some(s) => match TodoStatus::parse(s) {
                Ok(v) => Some(v),
                Err(e) => return IpcResponse::err(e.to_string()),
            },
        };
        match self
            .todos
            .update(&slug, &id, name, description, status_enum, status_reason)
            .await
        {
            Ok(todo) => {
                info!(slug = %slug, id = %todo.id, "AppTodosUpdate ok");
                IpcResponse::ok_data(serde_json::json!({ "todo": todo }))
            }
            Err(e) => {
                error!(slug = %slug, error = %e, "AppTodosUpdate failed");
                IpcResponse::err(format!("todos_update: {e}"))
            }
        }
    }

    pub async fn todos_delete(&self, slug: String, id: String) -> IpcResponse {
        if !valid_slug(&slug) {
            return IpcResponse::err("invalid slug");
        }
        match self.todos.delete(&slug, &id).await {
            Ok(()) => {
                info!(slug = %slug, id = %id, "AppTodosDelete ok");
                IpcResponse::ok_data(serde_json::json!({ "ok": true }))
            }
            Err(e) => {
                error!(slug = %slug, id = %id, error = %e, "AppTodosDelete failed");
                IpcResponse::err(format!("todos_delete: {e}"))
            }
        }
    }
}

impl AppsContext {
    /// Build an app remotely on the configured CloudMaster host.
    ///
    /// Steps :
    /// 1. SSH probe (fast-fail with actionable error if not configured).
    /// 2. `mkdir -p` the remote source dir.
    /// 3. `rsync` source up (excludes target/, node_modules/, .next/, dist/, .git/).
    /// 4. `ssh` the build command.
    /// 5. `rsync` the configured artefacts back.
    ///
    /// A per-slug lock prevents concurrent builds for the same app.
    /// The whole pipeline is bounded by `timeout_secs` (default 1800 = 30 min).
    #[tracing::instrument(skip(self), fields(slug = %slug))]
    pub async fn build(&self, slug: String, timeout_secs: Option<u64>) -> IpcResponse {
        if !valid_slug(&slug) {
            return IpcResponse::err("invalid slug");
        }
        let app = match self.supervisor.registry.get(&slug).await {
            Some(a) => a,
            None => return IpcResponse::err(format!("app not found: {slug}")),
        };

        let (build_command, default_artefacts) = match build_defaults_for_stack(&app) {
            Some(d) => d,
            None => {
                warn!(slug = %slug, stack = ?app.stack, "build: stack not supported");
                return IpcResponse::err(
                    "stack not supported by app.build; build manually".to_string(),
                );
            }
        };
        let build_command = app
            .build_command
            .clone()
            .unwrap_or_else(|| build_command.to_string());
        let artefacts: Vec<String> = match app.build_artefact.as_deref() {
            Some(custom) => custom
                .lines()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            None => default_artefacts,
        };
        if artefacts.is_empty() {
            return IpcResponse::err("no artefacts to rsync back (empty build_artefact)");
        }

        // ── Per-slug lock ───────────────────────────────────────────
        let lock = {
            let mut map = self.build_locks.lock().await;
            map.entry(slug.clone())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                .clone()
        };
        let _guard = match lock.try_lock() {
            Ok(g) => g,
            Err(_) => {
                warn!(slug = %slug, "build: already in progress");
                return IpcResponse::err(format!(
                    "BUILD_BUSY: another build for '{slug}' is already running. \
                     STOP and WAIT — do not retry automatically. \
                     Pause your work, inform the user that a concurrent build is in progress, \
                     and wait for the user to explicitly tell you to rebuild before calling app.build again."
                ));
            }
        };

        // Emit "started" event now that the lock is acquired.
        emit_build_event(
            &self.app_build_tx,
            &slug,
            "started",
            None,
            None,
            None,
            Some("build pipeline started".to_string()),
            None,
            None,
        );

        let host = std::env::var("HR_BUILD_HOST").unwrap_or_else(|_| BUILD_HOST.to_string());
        let key = std::env::var("HR_BUILD_SSH_KEY").unwrap_or_else(|_| SSH_KEY.to_string());
        let timeout = Duration::from_secs(timeout_secs.unwrap_or(1800).max(1));
        let started = Instant::now();
        let remote_src = format!("/opt/homeroute/apps/{}/src", slug);
        let local_src = app.src_dir();
        let local_src_str = format!("{}/", local_src.display());

        // SSH ControlMaster : multiplex all ssh/rsync calls of this build over a
        // single TCP connection to save ~200-300ms per call. Socket lives in
        // /tmp with slug + pid to avoid collisions between concurrent builds.
        let ctl_socket = format!("/tmp/hr-build-ssh-{}-{}.sock", slug, std::process::id());
        let ctl_path_opt = format!("ControlPath={ctl_socket}");
        let ssh_e_arg = format!(
            "ssh -i {key} -o BatchMode=yes -o StrictHostKeyChecking=accept-new \
             -o ControlMaster=auto -o {ctl_path_opt} -o ControlPersist=30 \
             -o ServerAliveInterval=10 -o ServerAliveCountMax=3"
        );

        info!(slug = %slug, host = %host, build_command = %build_command, timeout_secs = timeout.as_secs(), "build: start");

        let app_build_tx = self.app_build_tx.clone();
        let slug_for_pipeline = slug.clone();
        let supervisor_for_pipeline = self.supervisor.clone();
        let pipeline = async {
            let mut acc = StageAccumulator::new();
            let emit_step = |step: u32, phase: &str, dur_ms: u64, msg: Option<String>| {
                emit_build_event(
                    &app_build_tx,
                    &slug_for_pipeline,
                    "step",
                    Some(step),
                    Some(5),
                    Some(phase.to_string()),
                    msg,
                    Some(dur_ms),
                    None,
                );
            };

            if matches!(app.sources_on, SourcesLocation::CloudMaster) {
                // Sources canonical on CloudMaster — skip the rsync-up roundtrip.
                // We still emit the two "step" events the UI expects so the build
                // panel renders the full 5-step timeline consistently.
                info!(
                    slug = %slug,
                    sources_on = ?app.sources_on,
                    "build: skipping rsync-up, sources canonical on cloudmaster"
                );
                emit_step(
                    1,
                    "skipped:ssh-probe (sources on cloudmaster)",
                    0,
                    Some("sources on cloudmaster".to_string()),
                );
                emit_step(
                    2,
                    "skipped:rsync-up (sources on cloudmaster)",
                    0,
                    Some("sources on cloudmaster".to_string()),
                );
            } else {
                // 1) SSH probe
                info!(slug = %slug, host = %host, "build: ssh probe");
                let probe = run_capture(
                    "ssh",
                    &[
                        "-i", &key,
                        "-o", "BatchMode=yes",
                        "-o", "ConnectTimeout=5",
                        "-o", "StrictHostKeyChecking=accept-new",
                        "-o", "ControlMaster=auto",
                        "-o", &ctl_path_opt,
                        "-o", "ControlPersist=30", "-o", "ServerAliveInterval=10", "-o", "ServerAliveCountMax=3",
                        &host,
                        "true",
                    ],
                    None,
                )
                .await;
                acc.push("ssh-probe", &probe);
                emit_step(1, "ssh-probe", probe.duration_ms, None);
                if probe.exit_code != 0 {
                    error!(slug = %slug, exit_code = probe.exit_code, stderr = %truncate(&probe.stderr, 512), "build: ssh probe failed");
                    return acc.into_result(format!(
                        "ssh probe failed (host {host}); ensure SSH key {key} can log into CloudMaster (BatchMode)"
                    ), started);
                }

                // 2) mkdir remote
                info!(slug = %slug, remote_src = %remote_src, "build: mkdir remote");
                let mkdir = run_capture(
                    "ssh",
                    &[
                        "-i", &key,
                        "-o", "BatchMode=yes",
                        "-o", "StrictHostKeyChecking=accept-new",
                        "-o", "ControlMaster=auto",
                        "-o", &ctl_path_opt,
                        "-o", "ControlPersist=30", "-o", "ServerAliveInterval=10", "-o", "ServerAliveCountMax=3",
                        &host,
                        &format!("mkdir -p {}", shell_quote(&remote_src)),
                    ],
                    None,
                )
                .await;
                acc.push("mkdir", &mkdir);
                if mkdir.exit_code != 0 {
                    return acc.into_result("remote mkdir failed".into(), started);
                }

                // 3) rsync up
                info!(slug = %slug, "build: rsync up");
                let dest = format!("{}:{}/", host, remote_src);
                // LAN 10GbE: -W (whole-file) skips delta-xfer which is only useful on
                // slow networks; drop -z compression which caps throughput on CPU.
                let up = run_capture(
                    "rsync",
                    &[
                        "-a", "-W", "--delete",
                        "--exclude", "target/",
                        "--exclude", "node_modules/",
                        "--exclude", ".next/",
                        "--exclude", "dist/",
                        "--exclude", ".git/",
                        "-e", &ssh_e_arg,
                        &local_src_str,
                        &dest,
                    ],
                    None,
                )
                .await;
                acc.push("rsync-up", &up);
                emit_step(2, "rsync-up", up.duration_ms + mkdir.duration_ms, None);
                if up.exit_code != 0 {
                    return acc.into_result("rsync up failed".into(), started);
                }

                // Force ownership romain:romain so subsequent build (SSH'd as
                // romain) can write node_modules/, target/, etc. Tar-extracted
                // files preserve original (often root) ownership. No-op if
                // already correct.
                let chown_remote_app = format!("/opt/homeroute/apps/{}", slug);
                let chown_cmd = format!(
                    "chown -R romain:romain {}",
                    shell_quote(&chown_remote_app)
                );
                let chown = run_capture(
                    "ssh",
                    &[
                        "-i", &key,
                        "-o", "BatchMode=yes",
                        "-o", "StrictHostKeyChecking=accept-new",
                        "-o", "ControlMaster=auto",
                        "-o", &ctl_path_opt,
                        "-o", "ControlPersist=30",
                        &host,
                        &chown_cmd,
                    ],
                    None,
                )
                .await;
                if chown.exit_code != 0 {
                    warn!(
                        slug = %slug,
                        stderr = %truncate(&chown.stderr, 256),
                        "build: chown remote failed (non-fatal — build may EACCES)"
                    );
                }
            }

            // 4) build — wrap in `bash -lc` so the remote user's login shell
            // sources .profile / .cargo/env (otherwise cargo/rustup aren't in PATH).
            info!(slug = %slug, "build: compile (CI=true universal)");
            // Forcer CI=true pour pnpm/npm non-interactifs (sinon
            // ERR_PNPM_ABORTED_REMOVE_MODULES_DIR_NO_TTY). NPM_CONFIG_FUND=false
            // réduit le bruit. Variables exportées avant le `cd` pour persister
            // dans le subshell via `bash -lc`.
            let inner_cmd = format!(
                "export CI=true NPM_CONFIG_FUND=false && cd {} && {}",
                shell_quote(&remote_src),
                build_command
            );
            let remote_cmd = format!("bash -lc {}", shell_quote(&inner_cmd));
            let compile = run_capture(
                "ssh",
                &[
                    "-i", &key,
                    "-o", "BatchMode=yes",
                    "-o", "StrictHostKeyChecking=accept-new",
                    "-o", "ControlMaster=auto",
                    "-o", &ctl_path_opt,
                    "-o", "ControlPersist=30", "-o", "ServerAliveInterval=10", "-o", "ServerAliveCountMax=3",
                    &host,
                    &remote_cmd,
                ],
                None,
            )
            .await;
            acc.push("compile", &compile);
            emit_step(3, "compile", compile.duration_ms, None);
            if compile.exit_code != 0 {
                error!(slug = %slug, exit_code = compile.exit_code, "build: compile failed");
                return acc.into_result("build command failed".into(), started);
            }

            // 4b) stop the supervised process before overwriting artefacts on disk.
            // Avoids serving a partially-rsynced .next/, target/release binary, etc.
            // Best-effort: if the app is not running, this is a no-op.
            if let Err(e) = supervisor_for_pipeline.stop(&slug_for_pipeline).await {
                warn!(slug = %slug_for_pipeline, error = %e, "build: pre-rsync stop failed (continuing)");
            }

            // 5) rsync each artefact back
            let rsync_back_started = Instant::now();
            for art in &artefacts {
                info!(slug = %slug, artefact = %art, "build: rsync down");
                let remote_path = format!("{}/{}", remote_src, art);
                let local_path = local_src.join(art);
                if let Some(parent) = local_path.parent() {
                    let _ = tokio::fs::create_dir_all(parent).await;
                }
                // Existence check first to give a useful error.
                let exists = run_capture(
                    "ssh",
                    &[
                        "-i", &key,
                        "-o", "BatchMode=yes",
                        "-o", "StrictHostKeyChecking=accept-new",
                        "-o", "ControlMaster=auto",
                        "-o", &ctl_path_opt,
                        "-o", "ControlPersist=30", "-o", "ServerAliveInterval=10", "-o", "ServerAliveCountMax=3",
                        &host,
                        &format!("test -e {}", shell_quote(&remote_path)),
                    ],
                    None,
                )
                .await;
                if exists.exit_code != 0 {
                    acc.push(&format!("check-{art}"), &exists);
                    return acc.into_result(
                        format!("artefact missing on remote: {art}"),
                        started,
                    );
                }
                // Detect dir vs file on remote — for dirs we use trailing slash
                // + --delete to mirror exact contents. Without trailing slash
                // rsync nests the source dir INSIDE an existing dst dir
                // (observed: forge.next/.next/BUILD_ID instead of forge.next/BUILD_ID).
                let is_dir = run_capture(
                    "ssh",
                    &[
                        "-i", &key,
                        "-o", "BatchMode=yes",
                        "-o", "StrictHostKeyChecking=accept-new",
                        "-o", "ControlMaster=auto",
                        "-o", &ctl_path_opt,
                        "-o", "ControlPersist=30",
                        &host,
                        &format!("test -d {}", shell_quote(&remote_path)),
                    ],
                    None,
                )
                .await;
                let dir_mode = is_dir.exit_code == 0;
                let (src_arg, dst_arg, extra_args): (String, String, &[&str]) = if dir_mode {
                    let _ = tokio::fs::create_dir_all(&local_path).await;
                    (
                        format!("{}:{}/", host, remote_path),
                        format!("{}/", local_path.display()),
                        &["--delete"],
                    )
                } else {
                    (
                        format!("{}:{}", host, remote_path),
                        local_path.display().to_string(),
                        &[],
                    )
                };
                let mut rsync_args: Vec<&str> = vec!["-a", "-W"];
                rsync_args.extend_from_slice(extra_args);
                rsync_args.extend_from_slice(&["-e", &ssh_e_arg, &src_arg, &dst_arg]);
                let down = run_capture("rsync", &rsync_args, None).await;
                acc.push(&format!("rsync-down:{art}"), &down);
                if down.exit_code != 0 {
                    return acc.into_result(
                        format!("rsync down failed for {art}"),
                        started,
                    );
                }
            }
            emit_step(
                4,
                "rsync-back",
                rsync_back_started.elapsed().as_millis() as u64,
                None,
            );

            // 6) restart the app so the freshly rsynced artefacts are picked up.
            // Best-effort; if start fails we still consider the build OK and surface the warning.
            let restart_started = Instant::now();
            if let Err(e) = supervisor_for_pipeline.start(&slug_for_pipeline).await {
                warn!(slug = %slug_for_pipeline, error = %e, "build: post-rsync start failed");
                emit_step(
                    5,
                    "restart",
                    restart_started.elapsed().as_millis() as u64,
                    Some(format!("start failed: {e}")),
                );
            } else {
                emit_step(
                    5,
                    "restart",
                    restart_started.elapsed().as_millis() as u64,
                    None,
                );
            }

            info!(slug = %slug_for_pipeline, duration_ms = started.elapsed().as_millis() as u64, "build: ok");
            acc.into_result_ok(started)
        };

        let resp = match tokio::time::timeout(timeout, pipeline).await {
            Ok(r) => r,
            Err(_) => {
                error!(slug = %slug, timeout_secs = timeout.as_secs(), "build: timeout");
                IpcResponse::err(format!("build timed out after {}s", timeout.as_secs()))
            }
        };

        // Emit final build event: "finished" on success, "error" otherwise.
        let total_ms = started.elapsed().as_millis() as u64;
        if resp.ok {
            // The pipeline returns ok_data even when the inner command failed
            // (it stuffs an exit_code in AppExecResult). Inspect that to know.
            let exit_code = resp
                .data
                .as_ref()
                .and_then(|d| d.get("exit_code"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            if exit_code == 0 {
                emit_build_event(
                    &self.app_build_tx,
                    &slug,
                    "finished",
                    Some(5),
                    Some(5),
                    None,
                    Some("build finished".to_string()),
                    Some(total_ms),
                    None,
                );
            } else {
                let err_msg = resp
                    .data
                    .as_ref()
                    .and_then(|d| d.get("stderr"))
                    .and_then(|v| v.as_str())
                    .map(|s| truncate(s, 512))
                    .unwrap_or_else(|| "build failed".to_string());
                emit_build_event(
                    &self.app_build_tx,
                    &slug,
                    "error",
                    None,
                    Some(5),
                    None,
                    None,
                    Some(total_ms),
                    Some(err_msg),
                );
            }
        } else {
            let err_msg = resp.error.clone().unwrap_or_else(|| "build failed".into());
            emit_build_event(
                &self.app_build_tx,
                &slug,
                "error",
                None,
                Some(5),
                None,
                None,
                Some(total_ms),
                Some(err_msg),
            );
        }

        // Refresh the per-app context (build command may have changed).
        let all = self.supervisor.registry.list().await;
        let db_tables = if app.has_db {
            self.db_manager.list_tables(&slug).await.ok()
        } else {
            None
        };
        if let Err(e) = self.context_generator.generate_for_app(&app, &all, db_tables) {
            warn!(slug = %slug, error = %e, "build: context regen failed (non-fatal)");
        }

        resp
    }
}

/// Returns `(default_build_command, default_artefact_paths)` for stacks that
/// support remote build, or `None` for unsupported stacks.
fn build_defaults_for_stack(app: &Application) -> Option<(&'static str, Vec<String>)> {
    match app.stack {
        AppStack::Axum => Some((
            "cargo build --release",
            vec![format!("target/release/{}", app.slug)],
        )),
        AppStack::AxumVite => Some((
            "cargo build --release && (cd web && npm ci && npm run build)",
            vec![format!("target/release/{}", app.slug), "web/dist".to_string()],
        )),
        AppStack::NextJs => Some((
            "npm ci && npm run build",
            vec![
                ".next".to_string(),
                "public".to_string(),
                "package.json".to_string(),
                "package-lock.json".to_string(),
                "node_modules".to_string(),
            ],
        )),
        AppStack::Flutter => None,
    }
}

struct StageOutput {
    stdout: String,
    stderr: String,
    exit_code: i32,
    duration_ms: u64,
}

struct StageAccumulator {
    stdout: String,
    stderr: String,
    last_exit: i32,
    total_ms: u64,
}

impl StageAccumulator {
    fn new() -> Self {
        Self {
            stdout: String::new(),
            stderr: String::new(),
            last_exit: 0,
            total_ms: 0,
        }
    }

    fn push(&mut self, stage: &str, out: &StageOutput) {
        self.stdout.push_str(&format!("\n=== {stage} (exit={}, {}ms) ===\n", out.exit_code, out.duration_ms));
        self.stdout.push_str(&out.stdout);
        if !out.stderr.is_empty() {
            self.stderr.push_str(&format!("\n=== {stage} ===\n"));
            self.stderr.push_str(&out.stderr);
        }
        self.total_ms += out.duration_ms;
        if out.exit_code != 0 && self.last_exit == 0 {
            self.last_exit = out.exit_code;
        }
    }

    fn into_result(mut self, message: String, started: Instant) -> IpcResponse {
        if !self.stderr.is_empty() {
            self.stderr.push('\n');
        }
        self.stderr.push_str(&message);
        let exit = if self.last_exit == 0 { 1 } else { self.last_exit };
        let result = AppExecResult {
            stdout: cap_string(self.stdout),
            stderr: cap_string(self.stderr),
            exit_code: exit,
            duration_ms: started.elapsed().as_millis() as u64,
        };
        IpcResponse::ok_data(result)
    }

    fn into_result_ok(self, started: Instant) -> IpcResponse {
        let result = AppExecResult {
            stdout: cap_string(self.stdout),
            stderr: cap_string(self.stderr),
            exit_code: 0,
            duration_ms: started.elapsed().as_millis() as u64,
        };
        IpcResponse::ok_data(result)
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_build_event(
    tx: &broadcast::Sender<AppBuildEvent>,
    slug: &str,
    status: &str,
    step: Option<u32>,
    total_steps: Option<u32>,
    phase: Option<String>,
    message: Option<String>,
    duration_ms: Option<u64>,
    error: Option<String>,
) {
    let event = AppBuildEvent {
        slug: slug.to_string(),
        status: status.to_string(),
        step,
        total_steps,
        phase: phase.clone(),
        message,
        duration_ms,
        error: error.clone(),
    };
    info!(
        slug = %slug,
        status = %status,
        step = ?step,
        phase = ?phase,
        duration_ms = ?duration_ms,
        error = ?error,
        "AppBuildEvent emitted"
    );
    let _ = tx.send(event);
}

fn cap_string(mut s: String) -> String {
    if s.len() > OUTPUT_CAP_BYTES {
        let cut = OUTPUT_CAP_BYTES;
        // Snap to a char boundary
        let mut idx = cut;
        while idx > 0 && !s.is_char_boundary(idx) {
            idx -= 1;
        }
        s.truncate(idx);
        s.push_str("\n[truncated]\n");
    }
    s
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        let mut idx = n;
        while idx > 0 && !s.is_char_boundary(idx) {
            idx -= 1;
        }
        format!("{}…", &s[..idx])
    }
}

fn shell_quote(s: &str) -> String {
    // Single-quote everything; embed any internal single quotes.
    let escaped = s.replace('\'', "'\\''");
    format!("'{escaped}'")
}

async fn run_capture(program: &str, args: &[&str], cwd: Option<&std::path::Path>) -> StageOutput {
    let started = Instant::now();
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    let child = cmd.spawn();
    let child = match child {
        Ok(c) => c,
        Err(e) => {
            return StageOutput {
                stdout: String::new(),
                stderr: format!("spawn {program}: {e}"),
                exit_code: -1,
                duration_ms: started.elapsed().as_millis() as u64,
            };
        }
    };
    let out = match child.wait_with_output().await {
        Ok(o) => o,
        Err(e) => {
            return StageOutput {
                stdout: String::new(),
                stderr: format!("wait {program}: {e}"),
                exit_code: -1,
                duration_ms: started.elapsed().as_millis() as u64,
            };
        }
    };
    StageOutput {
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        exit_code: out.status.code().unwrap_or(-1),
        duration_ms: started.elapsed().as_millis() as u64,
    }
}

// ── Helpers ────────────────────────────────────────────────────

fn parse_stack(s: &str) -> Option<AppStack> {
    match s {
        "next-js" | "nextjs" => Some(AppStack::NextJs),
        "axum-vite" => Some(AppStack::AxumVite),
        "axum" => Some(AppStack::Axum),
        "flutter" => Some(AppStack::Flutter),
        _ => None,
    }
}

fn parse_visibility(s: &str) -> Option<Visibility> {
    match s {
        "public" => Some(Visibility::Public),
        "private" => Some(Visibility::Private),
        _ => None,
    }
}

fn stack_to_str(stack: &AppStack) -> &'static str {
    match stack {
        AppStack::NextJs => "next-js",
        AppStack::AxumVite => "axum-vite",
        AppStack::Axum => "axum",
        AppStack::Flutter => "flutter",
    }
}

fn visibility_to_str(v: &Visibility) -> &'static str {
    match v {
        Visibility::Public => "public",
        Visibility::Private => "private",
    }
}

fn state_to_str(s: &AppState) -> &'static str {
    match s {
        AppState::Stopped => "stopped",
        AppState::Starting => "starting",
        AppState::Running => "running",
        AppState::Stopping => "stopping",
        AppState::Crashed => "crashed",
        AppState::Unknown => "unknown",
    }
}

pub fn app_to_dto(app: &Application) -> ApplicationDto {
    ApplicationDto {
        slug: app.slug.clone(),
        name: app.name.clone(),
        description: app.description.clone(),
        stack: stack_to_str(&app.stack).to_string(),
        has_db: app.has_db,
        visibility: visibility_to_str(&app.visibility).to_string(),
        domain: app.domain.clone(),
        port: app.port,
        run_command: app.run_command.clone(),
        build_command: app.build_command.clone(),
        build_artefact: app.build_artefact.clone(),
        health_path: app.health_path.clone(),
        env_vars: app
            .env_vars
            .keys()
            .map(|k| (k.clone(), "***".to_string()))
            .collect(),
        state: state_to_str(&app.state).to_string(),
        sources_on: sources_location_to_str(&app.sources_on).to_string(),
        created_at: app.created_at.to_rfc3339(),
        updated_at: app.updated_at.to_rfc3339(),
    }
}

fn sources_location_to_str(s: &hr_apps::SourcesLocation) -> &'static str {
    match s {
        hr_apps::SourcesLocation::Medion => "medion",
        hr_apps::SourcesLocation::CloudMaster => "cloudmaster",
    }
}

fn process_status_to_dto(slug: &str, s: &ProcessStatus) -> AppStatusData {
    AppStatusData {
        slug: slug.to_string(),
        pid: s.pid,
        state: state_to_str(&s.state).to_string(),
        port: s.port,
        uptime_secs: s.uptime_secs,
        restart_count: s.restart_count,
    }
}

/// AppCreate path quand `sources_on == CloudMaster` :
/// scaffold dans un tmpdir local, puis rsync UP vers CloudMaster + chown.
#[tracing::instrument(skip(ctx_generator, all_apps), fields(slug = %app.slug))]
async fn scaffold_on_cloudmaster(
    app: &Application,
    ctx_generator: &ContextGenerator,
    all_apps: &[Application],
) -> anyhow::Result<()> {
    let host = std::env::var("HR_BUILD_HOST").unwrap_or_else(|_| BUILD_HOST.to_string());
    let key = std::env::var("HR_BUILD_SSH_KEY").unwrap_or_else(|_| SSH_KEY.to_string());
    let slug = app.slug.as_str();
    let remote_app_dir = format!("/opt/homeroute/apps/{slug}");
    let remote_src = format!("{remote_app_dir}/src");

    let tmp = PathBuf::from(format!(
        "/tmp/hr-scaffold-{slug}-{}",
        std::process::id()
    ));
    if tmp.exists() {
        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }
    tokio::fs::create_dir_all(&tmp).await?;
    info!(slug = %slug, tmp = %tmp.display(), "scaffold_on_cloudmaster: building locally before rsync");

    let scaffold_res: anyhow::Result<()> = async {
        scaffold::scaffold_stack_template_at(app, &tmp).await?;
        ctx_generator.generate_for_app_at(app, &tmp, all_apps, None, false)?;
        Ok(())
    }
    .await;
    if let Err(e) = scaffold_res {
        let _ = tokio::fs::remove_dir_all(&tmp).await;
        return Err(e.context("local scaffold failed"));
    }

    let mkdir_cmd = format!("mkdir -p {}", shell_quote(&remote_src));
    let mkdir = run_capture(
        "ssh",
        &[
            "-i", &key,
            "-o", "BatchMode=yes",
            "-o", "StrictHostKeyChecking=accept-new",
            "-o", "ConnectTimeout=10",
            &host,
            &mkdir_cmd,
        ],
        None,
    )
    .await;
    if mkdir.exit_code != 0 {
        let _ = tokio::fs::remove_dir_all(&tmp).await;
        anyhow::bail!(
            "remote mkdir failed (exit={}): {}",
            mkdir.exit_code,
            truncate(&mkdir.stderr, 256)
        );
    }

    let ssh_e_arg = format!(
        "ssh -i {key} -o BatchMode=yes -o StrictHostKeyChecking=accept-new"
    );
    let local_src_str = format!("{}/", tmp.display());
    let dest = format!("{host}:{remote_src}/");
    let up = run_capture(
        "rsync",
        &["-a", "-W", "-e", &ssh_e_arg, &local_src_str, &dest],
        None,
    )
    .await;
    if up.exit_code != 0 {
        let _ = tokio::fs::remove_dir_all(&tmp).await;
        anyhow::bail!(
            "rsync up failed (exit={}): {}",
            up.exit_code,
            truncate(&up.stderr, 256)
        );
    }

    let chown_cmd = format!(
        "chown -R romain:romain {}",
        shell_quote(&remote_app_dir)
    );
    let chown = run_capture(
        "ssh",
        &[
            "-i", &key,
            "-o", "BatchMode=yes",
            "-o", "StrictHostKeyChecking=accept-new",
            &host,
            &chown_cmd,
        ],
        None,
    )
    .await;
    if chown.exit_code != 0 {
        warn!(
            slug = %slug,
            stderr = %truncate(&chown.stderr, 256),
            "scaffold_on_cloudmaster: chown romain failed (non-fatal — build SSH may not be able to write)"
        );
    }

    let _ = tokio::fs::remove_dir_all(&tmp).await;

    info!(
        slug = %slug,
        host = %host,
        remote_src = %remote_src,
        "scaffold_on_cloudmaster: done"
    );
    Ok(())
}

/// AppRegenerateContext path quand `sources_on == CloudMaster` :
/// régénère CLAUDE.md / .claude/ / .mcp.json dans un tmpdir local puis
/// rsync UP vers CloudMaster sans toucher au reste du src/. On utilise
/// `--include` pour ne pousser que les fichiers de contexte.
#[tracing::instrument(skip(ctx_generator, all_apps, db_tables), fields(slug = %app.slug))]
async fn regen_context_on_cloudmaster(
    app: &Application,
    ctx_generator: &ContextGenerator,
    all_apps: &[Application],
    db_tables: Option<Vec<String>>,
) -> anyhow::Result<()> {
    let host = std::env::var("HR_BUILD_HOST").unwrap_or_else(|_| BUILD_HOST.to_string());
    let key = std::env::var("HR_BUILD_SSH_KEY").unwrap_or_else(|_| SSH_KEY.to_string());
    let slug = app.slug.as_str();
    let remote_src = format!("/opt/homeroute/apps/{slug}/src");

    let tmp = PathBuf::from(format!(
        "/tmp/hr-regen-{slug}-{}",
        std::process::id()
    ));
    if tmp.exists() {
        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }
    tokio::fs::create_dir_all(&tmp).await?;

    let res = ctx_generator.generate_for_app_at(app, &tmp, all_apps, db_tables, false);
    if let Err(e) = res {
        let _ = tokio::fs::remove_dir_all(&tmp).await;
        return Err(e.context("local regen failed"));
    }

    // Rsync UP only the agent context files, not the whole tmpdir.
    // -R is implicit through trailing slash semantics; we filter via --include.
    let ssh_e_arg = format!(
        "ssh -i {key} -o BatchMode=yes -o StrictHostKeyChecking=accept-new"
    );
    let local_src_str = format!("{}/", tmp.display());
    let dest = format!("{host}:{remote_src}/");
    let up = run_capture(
        "rsync",
        &[
            "-a", "-W",
            "--include=CLAUDE.md",
            "--include=.mcp.json",
            "--include=.claude/",
            "--include=.claude/**",
            "--exclude=*",
            "-e", &ssh_e_arg,
            &local_src_str,
            &dest,
        ],
        None,
    )
    .await;

    let _ = tokio::fs::remove_dir_all(&tmp).await;

    if up.exit_code != 0 {
        anyhow::bail!(
            "rsync regen up failed (exit={}): {}",
            up.exit_code,
            truncate(&up.stderr, 256)
        );
    }

    info!(
        slug = %slug,
        host = %host,
        remote_src = %remote_src,
        "regen_context_on_cloudmaster: done"
    );
    Ok(())
}

/// AppDelete path quand `sources_on == CloudMaster` :
/// supprime le dossier de l'app sur CloudMaster (src/ + tout le reste).
#[tracing::instrument]
async fn cleanup_cloudmaster_src(slug: &str) -> anyhow::Result<()> {
    let host = std::env::var("HR_BUILD_HOST").unwrap_or_else(|_| BUILD_HOST.to_string());
    let key = std::env::var("HR_BUILD_SSH_KEY").unwrap_or_else(|_| SSH_KEY.to_string());
    let remote_app_dir = format!("/opt/homeroute/apps/{slug}");
    let cmd = format!("rm -rf {}", shell_quote(&remote_app_dir));

    let rm = run_capture(
        "ssh",
        &[
            "-i", &key,
            "-o", "BatchMode=yes",
            "-o", "StrictHostKeyChecking=accept-new",
            "-o", "ConnectTimeout=10",
            &host,
            &cmd,
        ],
        None,
    )
    .await;
    if rm.exit_code != 0 {
        anyhow::bail!(
            "remote rm -rf failed (exit={}): {}",
            rm.exit_code,
            truncate(&rm.stderr, 256)
        );
    }

    info!(
        slug = %slug,
        host = %host,
        remote_app_dir = %remote_app_dir,
        "cleanup_cloudmaster_src: done"
    );
    Ok(())
}

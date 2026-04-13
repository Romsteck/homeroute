//! IPC handlers for `App*` variants (hr-apps integration).
//!
//! Split out of `ipc_handler.rs` to keep that file manageable.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::scaffold;

use hr_apps::types::{AppStack, AppState, Application, Visibility, valid_slug};
use hr_apps::{AppSupervisor, ContextGenerator, DbManager, ProcessStatus};
use hr_common::logging::{LogQuery, LogStore};
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
    pub context_generator: Arc<ContextGenerator>,
    pub edge: Arc<EdgeClient>,
    pub git: Arc<hr_git::GitService>,
    pub base_domain: String,
    pub log_store: Arc<LogStore>,
    /// Per-slug locks to serialise concurrent `build()` invocations.
    pub build_locks:
        Arc<tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
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

        // Ensure filesystem layout exists
        let app_dir = app.app_dir();
        if let Err(e) = tokio::fs::create_dir_all(&app_dir).await {
            error!(slug = %slug, error = %e, "AppCreate: create app_dir failed");
            return IpcResponse::err(format!("create app dir failed: {e}"));
        }
        if let Err(e) = tokio::fs::create_dir_all(&app.src_dir()).await {
            warn!(slug = %slug, error = %e, "AppCreate: create src_dir failed");
        }

        // Scaffold a minimal source tree (idempotent — never overwrites).
        if let Err(e) = scaffold::scaffold_stack_template(&app).await {
            warn!(slug = %slug, error = %e, "AppCreate: scaffold template failed (non-fatal)");
        }
        // Apply a sensible default `run_command` if the caller did not provide one.
        if app.run_command.trim().is_empty() {
            app.run_command = scaffold::default_run_command(&app);
            info!(slug = %slug, run_command = %app.run_command, "AppCreate: applied default run_command");
        }
        if has_db {
            // Touch the db file so it exists
            let _ = tokio::fs::File::create(app.db_path()).await;
        }

        // Persist app
        if let Err(e) = self.supervisor.registry.upsert(app.clone()).await {
            self.supervisor.port_registry.release(&slug).await.ok();
            error!(slug = %slug, error = %e, "AppCreate: registry upsert failed");
            return IpcResponse::err(format!("registry upsert failed: {e}"));
        }

        // hr-git bare repo (best-effort)
        if let Err(e) = self.git.create_repo(&slug).await {
            warn!(slug = %slug, error = %e, "AppCreate: git create_repo failed (non-fatal)");
        }

        // hr-edge route (best-effort)
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
            warn!(slug = %slug, error = %e, "AppCreate: context generation failed (non-fatal)");
        }
        if let Err(e) = self.context_generator.generate_root(&all) {
            warn!(error = %e, "AppCreate: root context generation failed (non-fatal)");
        }

        info!(slug = %slug, port, duration_ms = start.elapsed().as_millis() as u64, "AppCreate ok");
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

    pub async fn delete(&self, slug: String, keep_data: bool) -> IpcResponse {
        if !valid_slug(&slug) {
            return IpcResponse::err("invalid slug");
        }
        let app = match self.supervisor.registry.get(&slug).await {
            Some(a) => a,
            None => return IpcResponse::err(format!("app not found: {slug}")),
        };

        if let Err(e) = self.supervisor.stop(&slug).await {
            warn!(slug = %slug, error = %e, "AppDelete: stop failed (continuing)");
        }
        if let Err(e) = self.edge.remove_app_route(&app.domain).await {
            warn!(slug = %slug, domain = %app.domain, error = %e, "AppDelete: edge remove_app_route failed");
        }
        if let Err(e) = self.supervisor.registry.remove(&slug).await {
            error!(slug = %slug, error = %e, "AppDelete: registry remove failed");
            return IpcResponse::err(format!("registry remove failed: {e}"));
        }
        if let Err(e) = self.supervisor.port_registry.release(&slug).await {
            warn!(slug = %slug, error = %e, "AppDelete: port release failed");
        }
        if !keep_data {
            let dir: PathBuf = PathBuf::from(format!("/opt/homeroute/apps/{}", slug));
            if let Err(e) = tokio::fs::remove_dir_all(&dir).await {
                warn!(slug = %slug, dir = %dir.display(), error = %e, "AppDelete: rm -rf failed");
            }
        }

        // Regenerate root context
        let all = self.supervisor.registry.list().await;
        if let Err(e) = self.context_generator.generate_root(&all) {
            warn!(error = %e, "AppDelete: root context regeneration failed");
        }

        info!(slug = %slug, keep_data, "AppDelete ok");
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
            "rebuild" => {
                // TODO(V3C): run app.build_command then restart
                match self.supervisor.registry.get(&slug).await {
                    Some(app) => {
                        if let Some(cmd) = app.build_command.clone() {
                            info!(slug = %slug, "AppControl rebuild: running build_command");
                            let out = tokio::process::Command::new("/bin/bash")
                                .arg("-c")
                                .arg(&cmd)
                                .current_dir(app.src_dir())
                                .output()
                                .await;
                            match out {
                                Ok(o) if o.status.success() => self.supervisor.restart(&slug).await,
                                Ok(o) => Err(anyhow::anyhow!(
                                    "build failed: {}",
                                    String::from_utf8_lossy(&o.stderr)
                                )),
                                Err(e) => Err(anyhow::anyhow!("spawn build: {e}")),
                            }
                        } else {
                            self.supervisor.restart(&slug).await
                        }
                    }
                    None => Err(anyhow::anyhow!("app not found: {slug}")),
                }
            }
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
        let filter = LogQuery {
            q: Some(slug.clone()),
            limit: Some(limit.unwrap_or(200) as u32),
            level: level.map(|l| vec![l.parse().unwrap_or(hr_common::logging::LogLevel::Info)]),
            ..Default::default()
        };
        match self.log_store.query(&filter).await {
            Ok(entries) => {
                let logs: Vec<AppLogEntry> = entries
                    .into_iter()
                    .map(|e| AppLogEntry {
                        timestamp: e.timestamp.to_rfc3339(),
                        level: format!("{:?}", e.level).to_lowercase(),
                        message: e.message,
                        data: e.data,
                    })
                    .collect();
                info!(slug = %slug, count = logs.len(), "AppLogs queried");
                IpcResponse::ok_data(AppLogsData { slug, logs })
            }
            Err(e) => {
                error!(slug = %slug, error = %e, "AppLogs query failed");
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
        if let Err(e) = self
            .context_generator
            .generate_for_app(&app, &all, db_tables)
        {
            error!(slug = %slug, error = %e, "AppRegenerateContext failed");
            return IpcResponse::err(format!("generate_for_app: {e}"));
        }
        if let Err(e) = self.context_generator.generate_root(&all) {
            warn!(error = %e, "AppRegenerateContext root failed");
        }
        info!(slug = %slug, "AppRegenerateContext ok");
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
             -o ControlMaster=auto -o {ctl_path_opt} -o ControlPersist=30"
        );

        info!(slug = %slug, host = %host, build_command = %build_command, timeout_secs = timeout.as_secs(), "build: start");

        let pipeline = async {
            let mut acc = StageAccumulator::new();

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
                    "-o", "ControlPersist=30",
                    &host,
                    "true",
                ],
                None,
            )
            .await;
            acc.push("ssh-probe", &probe);
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
                    "-o", "ControlPersist=30",
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
            if up.exit_code != 0 {
                return acc.into_result("rsync up failed".into(), started);
            }

            // 4) build — wrap in `bash -lc` so the remote user's login shell
            // sources .profile / .cargo/env (otherwise cargo/rustup aren't in PATH).
            info!(slug = %slug, "build: compile");
            let inner_cmd = format!(
                "cd {} && {}",
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
                    "-o", "ControlPersist=30",
                    &host,
                    &remote_cmd,
                ],
                None,
            )
            .await;
            acc.push("compile", &compile);
            if compile.exit_code != 0 {
                error!(slug = %slug, exit_code = compile.exit_code, "build: compile failed");
                return acc.into_result("build command failed".into(), started);
            }

            // 5) rsync each artefact back
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
                        "-o", "ControlPersist=30",
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
                let src_arg = format!("{}:{}", host, remote_path);
                let dst_arg = local_path.display().to_string();
                let down = run_capture(
                    "rsync",
                    &["-a", "-W", "-e", &ssh_e_arg, &src_arg, &dst_arg],
                    None,
                )
                .await;
                acc.push(&format!("rsync-down:{art}"), &down);
                if down.exit_code != 0 {
                    return acc.into_result(
                        format!("rsync down failed for {art}"),
                        started,
                    );
                }
            }

            info!(slug = %slug, duration_ms = started.elapsed().as_millis() as u64, "build: ok");
            acc.into_result_ok(started)
        };

        let resp = match tokio::time::timeout(timeout, pipeline).await {
            Ok(r) => r,
            Err(_) => {
                error!(slug = %slug, timeout_secs = timeout.as_secs(), "build: timeout");
                IpcResponse::err(format!("build timed out after {}s", timeout.as_secs()))
            }
        };

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
        created_at: app.created_at.to_rfc3339(),
        updated_at: app.updated_at.to_rfc3339(),
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

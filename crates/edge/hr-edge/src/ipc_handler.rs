use std::sync::Arc;

use hr_ipc::edge::EdgeRequest;
use hr_ipc::server::IpcHandler;
use hr_ipc::types::IpcResponse;
use tracing::{info, warn};

use crate::dns_route_sync::DnsRouteSync;

pub struct EdgeHandler {
    pub auth: Arc<hr_auth::AuthService>,
    pub acme: Arc<hr_acme::AcmeManager>,
    pub proxy: Arc<hr_proxy::ProxyState>,
    pub tls_manager: Arc<hr_proxy::TlsManager>,
    pub env: Arc<hr_common::config::EnvConfig>,
    pub dns_route_sync: Arc<DnsRouteSync>,
}

impl EdgeHandler {
    /// Re-load ACME wildcard certificates into the TLS manager.
    /// Must be called after `reload_certificates` which does `replace_all`
    /// and would otherwise wipe ACME certs loaded at startup.
    fn reload_acme_certs(&self) {
        let certs = self.acme.list_certificates().unwrap_or_default();
        let mut loaded = 0;
        for cert_info in &certs {
            let cert_path = std::path::Path::new(&cert_info.cert_path);
            let key_path = std::path::Path::new(&cert_info.key_path);
            if cert_path.exists() && key_path.exists() {
                match self.tls_manager.load_cert_from_files(cert_path, key_path) {
                    Ok(certified_key) => {
                        let domain = cert_info
                            .wildcard_type
                            .domain_pattern(&self.env.base_domain);
                        self.tls_manager.add_cert(&domain, certified_key);
                        loaded += 1;
                    }
                    Err(e) => {
                        warn!(cert_id = %cert_info.id, error = %e, "Failed to reload ACME cert");
                    }
                }
            }
        }
        // Re-set fallback cert
        if let Ok(cert_info) = self.acme.get_certificate(hr_acme::WildcardType::Global) {
            if let Err(e) = self
                .tls_manager
                .set_fallback_certificate_from_pem(&cert_info.cert_path, &cert_info.key_path)
            {
                warn!("Failed to re-set fallback certificate: {}", e);
            }
        }
        info!("Re-loaded {} ACME certificates after config reload", loaded);
    }
}

impl IpcHandler<EdgeRequest, IpcResponse> for EdgeHandler {
    async fn handle(&self, request: EdgeRequest) -> IpcResponse {
        match request {
            // ── Route management ──────────────────────────────────
            EdgeRequest::SetAppRoute {
                domain,
                app_id,
                host_id,
                target_ip,
                target_port,
                auth_required,
                allowed_groups,
                local_only,
            } => {
                let ip: std::net::Ipv4Addr = match target_ip.parse() {
                    Ok(ip) => ip,
                    Err(e) => return IpcResponse::err(format!("Invalid IP: {}", e)),
                };
                self.proxy.set_app_route(
                    domain,
                    hr_proxy::AppRoute {
                        app_id,
                        host_id,
                        target_ip: ip,
                        target_port,
                        auth_required,
                        allowed_groups,
                        local_only,
                    },
                );
                self.dns_route_sync.request_sync();
                IpcResponse::ok_empty()
            }
            EdgeRequest::RemoveAppRoute { domain } => {
                self.proxy.remove_app_route(&domain);
                self.dns_route_sync.request_sync();
                IpcResponse::ok_empty()
            }
            EdgeRequest::ListAppRoutes => IpcResponse::ok_data(self.proxy.list_app_routes()),

            // ── Proxy config ──────────────────────────────────────
            EdgeRequest::ReloadConfig => {
                match hr_proxy::ProxyConfig::load_from_file(&self.env.proxy_config_path) {
                    Ok(new_config) => {
                        if let Err(e) = self.tls_manager.reload_certificates(&new_config.routes) {
                            return IpcResponse::err(format!("TLS reload failed: {}", e));
                        }
                        self.reload_acme_certs();
                        self.proxy.reload_config(new_config);
                        self.dns_route_sync.request_sync();
                        IpcResponse::ok_empty()
                    }
                    Err(e) => IpcResponse::err(format!("Config reload failed: {}", e)),
                }
            }
            EdgeRequest::GetProxyConfig => IpcResponse::ok_data(self.proxy.config()),
            EdgeRequest::SaveProxyConfig { config } => {
                match serde_json::to_string_pretty(&config) {
                    Ok(json) => {
                        if let Err(e) = std::fs::write(&self.env.proxy_config_path, &json) {
                            return IpcResponse::err(format!("Failed to write config: {}", e));
                        }
                        // Trigger reload after save
                        match hr_proxy::ProxyConfig::load_from_file(&self.env.proxy_config_path) {
                            Ok(new_config) => {
                                if let Err(e) =
                                    self.tls_manager.reload_certificates(&new_config.routes)
                                {
                                    return IpcResponse::err(format!("TLS reload failed: {}", e));
                                }
                                self.reload_acme_certs();
                                self.proxy.reload_config(new_config);
                                self.dns_route_sync.request_sync();
                                IpcResponse::ok_empty()
                            }
                            Err(e) => IpcResponse::err(format!("Config reload failed: {}", e)),
                        }
                    }
                    Err(e) => IpcResponse::err(format!("Invalid config JSON: {}", e)),
                }
            }

            // ── ACME ──────────────────────────────────────────────
            EdgeRequest::AcmeStatus => IpcResponse::ok_data(serde_json::json!({
                "initialized": self.acme.is_initialized(),
            })),
            EdgeRequest::AcmeListCertificates => match self.acme.list_certificates() {
                Ok(certs) => IpcResponse::ok_data(certs),
                Err(e) => IpcResponse::err(format!("{}", e)),
            },
            EdgeRequest::AcmeRequestAppWildcard { slug: _ } => IpcResponse::err(
                "Per-app wildcard certs have been removed. Only the global wildcard is issued.",
            ),
            EdgeRequest::AcmeRequestEnvWildcard { env_slug: _ } => IpcResponse::err(
                "Per-env wildcard certs have been removed. Only the global wildcard is issued.",
            ),
            EdgeRequest::AcmeRenewAll => IpcResponse::ok_data("renewal triggered"),

            // ── Auth ──────────────────────────────────────────────
            EdgeRequest::AuthLogin {
                username,
                password,
                client_ip,
            } => {
                // Look up user with password hash, then verify
                match self.auth.users.get_with_password(&username) {
                    Some(user) => {
                        if hr_auth::users::verify_password(&password, &user.password_hash) {
                            self.auth.users.update_last_login(&username);
                            match self.auth.sessions.create(
                                &username,
                                Some(&client_ip),
                                None,
                                false,
                            ) {
                                Ok((token, expires_at)) => {
                                    IpcResponse::ok_data(serde_json::json!({
                                        "token": token,
                                        "expires_at": expires_at,
                                        "username": username,
                                    }))
                                }
                                Err(e) => {
                                    IpcResponse::err(format!("Session creation failed: {}", e))
                                }
                            }
                        } else {
                            IpcResponse::err("Invalid credentials")
                        }
                    }
                    None => IpcResponse::err("Invalid credentials"),
                }
            }
            EdgeRequest::AuthLogout { session_token } => {
                let _ = self.auth.sessions.delete(&session_token);
                IpcResponse::ok_empty()
            }
            EdgeRequest::AuthValidateSession { session_token } => {
                match self.auth.sessions.validate(&session_token) {
                    Ok(Some(session)) => IpcResponse::ok_data(session),
                    Ok(None) => IpcResponse::err("Invalid session"),
                    Err(e) => IpcResponse::err(format!("Session lookup failed: {}", e)),
                }
            }
            EdgeRequest::AuthListSessions => {
                // SessionStore does not have list_all; not implemented yet
                IpcResponse::err("Not implemented: list sessions")
            }
            EdgeRequest::AuthListUsers => {
                // UserStore does not have a list method; not implemented yet
                IpcResponse::err("Not implemented: list users")
            }
            EdgeRequest::AuthCreateUser {
                username: _,
                password: _,
                groups: _,
            } => {
                // UserStore does not have a create method; not implemented yet
                IpcResponse::err("Not implemented: create user")
            }
            EdgeRequest::AuthDeleteUser { username: _ } => {
                // UserStore does not have a delete method; not implemented yet
                IpcResponse::err("Not implemented: delete user")
            }
            EdgeRequest::AuthChangePassword {
                username,
                old_password,
                new_password,
            } => {
                // Verify old password first
                match self.auth.users.get_with_password(&username) {
                    Some(user) => {
                        if hr_auth::users::verify_password(&old_password, &user.password_hash) {
                            match self.auth.users.change_password(&username, &new_password) {
                                Ok(()) => IpcResponse::ok_empty(),
                                Err(e) => IpcResponse::err(e),
                            }
                        } else {
                            IpcResponse::err("Invalid current password")
                        }
                    }
                    None => IpcResponse::err("User not found"),
                }
            }

            // ── Stats / metrics ────────────────────────────────
            EdgeRequest::GetStats => {
                let global = self.proxy.metrics.global_snapshot();
                let domain_stats = self.proxy.metrics.snapshot();
                let cert_expiry = self.acme.cert_expiry_info().unwrap_or_default();
                IpcResponse::ok_data(serde_json::json!({
                    "global": global,
                    "domains": domain_stats,
                    "certificates": cert_expiry,
                }))
            }
        }
    }
}

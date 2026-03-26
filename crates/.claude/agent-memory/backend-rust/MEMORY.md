# Backend Rust Agent Memory

## Architecture (3 binaries as of 2026-03-08)

- **hr-netcore**: DNS, DHCP, Adblock, IPv6 (stable, rarely restarts)
- **hr-edge**: Proxy HTTPS, TLS/SNI, ACME, Auth
- **hr-orchestrator**: Containers, AgentRegistry, Git, Dataverse
- **homeroute**: API gateway (axum on port 4000), delegates to edge/orchestrator/netcore via IPC
- IPC sockets: `/run/hr-netcore.sock`, `/run/hr-edge.sock`, `/run/hr-orchestrator.sock`

## IPC Pattern

- Protocol: JSON-line over Unix socket (one connection per request)
- Server: `hr_ipc::server::IpcHandler` trait + `run_ipc_server()`
- Client: dedicated structs (`EdgeClient`, `OrchestratorClient`, `NetcoreClient`) in `hr-ipc`
- Request enums use `#[serde(tag = "cmd", rename_all = "snake_case")]`
- Response: `IpcResponse { ok, error, data }` -- same type for all services

## Key Crate Paths

- Registry methods: `hr-registry/src/state.rs` (AgentRegistry)
- Container manager: `hr-orchestrator/src/container_manager.rs` (canonical copy)
  - Original also in `hr-api/src/container_manager.rs` (TODO: deduplicate)
- Git service: `hr-git/src/service.rs`
- IPC types: `hr-ipc/src/types.rs` (IpcResponse), `hr-ipc/src/edge.rs`, `hr-ipc/src/orchestrator.rs`

## Build Rules

- Build only: `make server`, `make orchestrator`, `make edge`, `make netcore` (safe on dev)
- Deploy: `make deploy-prod` from dev (never `make deploy` on dev)
- Tests: `cargo test` in `/opt/homeroute/crates/`
- NEVER `cargo run` -- use systemd

## Conventions

- `UpdateApplicationRequest` derives `Default` -- safe to use `..Default::default()`
- `FrontendEndpoint` does NOT derive `Default`
- `#[allow(dead_code)]` on module declaration in main.rs for copied modules with future methods

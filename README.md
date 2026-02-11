# HomeRoute

A unified home server gateway that manages all network services from a single Rust binary. DNS, DHCP, HTTPS reverse proxy, ad-blocking, certificate management, container orchestration, and more — with a React web dashboard and an Android app store client.

## Features

- **DNS Server** — Recursive resolver with caching, upstream forwarding (Cloudflare, Google), query logging, and ad-block integration (UDP/TCP port 53)
- **DHCP Server** — DHCPv4 with DORA handshake, static leases, and JSON-persisted lease store (port 67)
- **IPv6** — Router Advertisement (RA), stateless DHCPv6, and prefix delegation (DHCP-PD)
- **HTTPS Reverse Proxy** — TLS termination with SNI routing, WebSocket support, forward-auth, and access logging (ports 80/443)
- **Ad-Blocking** — DNS-level domain filtering with configurable blocklists and whitelist
- **ACME Certificates** — Automatic Let's Encrypt wildcard certificates via Cloudflare DNS-01 challenges
- **Container Management** — systemd-nspawn containers with agent deployment, metrics, live migration, and auto-updates
- **Cloud Relay** — QUIC tunnel gateway for remote access without port forwarding
- **Dynamic DNS** — Cloudflare DDNS with automatic IPv6/IPv4 sync (direct or relay mode)
- **Dataverse** — Schema-driven data engine with migrations, queries, and per-app storage
- **App Store** — Backend catalog API with release management + Expo Android client
- **Authentication** — Session-based auth (SQLite + Argon2id), YAML user store, forward-auth middleware
- **Multi-Host** — Host agent protocol for managing multiple machines via WebSocket

## Tech Stack

**Backend**
- Rust (Cargo workspace, 17 crates)
- Axum (HTTP/WebSocket API)
- Tokio (async runtime)
- rustls / tokio-rustls (TLS)
- Quinn (QUIC tunneling)
- rusqlite (SQLite sessions)

**Frontend — Web Dashboard** (`web/`)
- React 18 with React Router 6
- Vite 5
- Tailwind CSS 3
- Axios + Recharts

**Frontend — App Store** (`store/`)
- Expo / React Native (Android)
- React Navigation
- Expo SecureStore

## Architecture

```
┌──────────────────────────────────────────────────┐
│                 homeroute (binary)                │
│           Supervisor + Service Registry           │
├──────────┬──────────┬──────────┬─────────────────┤
│ hr-proxy │  hr-dns  │ hr-dhcp  │    hr-ipv6      │
│ :443/:80 │   :53    │   :67    │   RA/DHCPv6     │
├──────────┴──────────┴──────────┴─────────────────┤
│  hr-api (:4000)  │  hr-auth  │  hr-adblock       │
│  REST + WebSocket │  SQLite   │  Domain filter    │
├──────────────────┼───────────┼───────────────────┤
│  hr-acme         │ hr-registry │ hr-container     │
│  Let's Encrypt   │ Agent mgmt  │ nspawn lifecycle │
├──────────────────┼─────────────┼─────────────────┤
│  hr-tunnel       │ hr-cloud-relay │ hr-dataverse  │
│  QUIC protocol   │ Remote gateway │ Data engine   │
├──────────────────┴─────────────┴─────────────────┤
│  hr-agent (in containers)  │  hr-host-agent      │
│  Metrics, MCP, auto-update │  Native host agent   │
└────────────────────────────┴─────────────────────┘
```

### Cargo Workspace

```
crates/
├── homeroute/         # Main binary — supervisor, service orchestration
├── hr-common/         # Shared types, EnvConfig, EventBus
├── hr-api/            # Axum HTTP router, REST + WebSocket endpoints
├── hr-auth/           # Authentication (SQLite sessions, YAML users, Argon2id)
├── hr-proxy/          # HTTPS reverse proxy (TLS/SNI, WebSocket, forward-auth)
├── hr-dns/            # DNS server (UDP/TCP, cache, upstream, adblock integration)
├── hr-dhcp/           # DHCP server (DHCPv4, DORA, lease persistence)
├── hr-ipv6/           # IPv6 RA + DHCPv6 stateless + prefix delegation
├── hr-adblock/        # Ad-block engine (domain filter, blocklists, whitelist)
├── hr-acme/           # ACME certificates (Let's Encrypt, Cloudflare DNS-01)
├── hr-firewall/       # IPv6 firewall (nftables)
├── hr-container/      # systemd-nspawn container client
├── hr-registry/       # Agent registry, metrics, Cloudflare DNS sync
├── hr-agent/          # Agent binary deployed inside nspawn containers
├── hr-host-agent/     # Host-level agent for native services
├── hr-tunnel/         # QUIC tunnel protocol + crypto
├── hr-cloud-relay/    # Cloud relay gateway (QUIC + TCP)
└── hr-dataverse/      # Data engine (schema, queries, migrations)
```

## Ports

| Port | Service | Protocol |
|------|---------|----------|
| 443 | HTTPS reverse proxy | TCP |
| 80 | HTTP → HTTPS redirect | TCP |
| 53 | DNS server | UDP/TCP |
| 67 | DHCP server | UDP |
| 4000 | Management API | TCP (HTTP + WebSocket) |
| 4443 | Cloud relay | QUIC |

## Build & Deploy

```bash
# Prerequisites: Rust toolchain, Node.js 22+

# Full build (server + frontend)
make all

# Deploy (build + restart systemd service)
make deploy

# Server only
make server

# Frontend only
make web

# Run tests
make test

# Clean
make clean
```

## Service Management

```bash
# Start/restart
systemctl restart homeroute

# Hot-reload proxy config (SIGHUP)
systemctl reload homeroute

# Logs
journalctl -u homeroute -f

# Health check
curl -s http://localhost:4000/api/health | jq
```

## Configuration

Environment variables in `.env`:

```bash
API_PORT=4000
BASE_DOMAIN=example.com
AUTH_DATA_DIR=/opt/homeroute/data
DATA_DIR=/opt/homeroute/data
WEB_DIST_PATH=/opt/homeroute/web/dist
ACME_STORAGE_PATH=/var/lib/server-dashboard/acme

# Cloudflare
CF_API_TOKEN=...
CF_ZONE_ID=...
CF_RECORD_NAME=...
CF_INTERFACE=eth0

# Cloud Relay
CLOUD_RELAY_ENABLED=false
CLOUD_RELAY_HOST=...
CLOUD_RELAY_QUIC_PORT=4443
```

## Data Storage

| Data | Format | Path |
|------|--------|------|
| Sessions | SQLite | `data/auth.db` |
| Users | YAML | `data/users.yml` |
| Hosts | JSON | `data/hosts.json` |
| Agent registry | JSON | `/var/lib/server-dashboard/agent-registry.json` |
| Proxy config | JSON | `/var/lib/server-dashboard/rust-proxy-config.json` |
| DNS/DHCP config | JSON | `/var/lib/server-dashboard/dns-dhcp-config.json` |
| Reverse proxy config | JSON | `/var/lib/server-dashboard/reverseproxy-config.json` |
| ACME certificates | PEM | `/var/lib/server-dashboard/acme/` |
| DHCP leases | JSON | `/var/lib/server-dashboard/dhcp-leases` |

## API Endpoints

| Route | Description |
|-------|-------------|
| `/api/auth` | Login, logout, sessions, forward-auth |
| `/api/dns-dhcp` | DNS/DHCP configuration and leases |
| `/api/adblock` | Ad-blocking stats and whitelist |
| `/api/ddns` | Dynamic DNS status and sync |
| `/api/reverseproxy` | Reverse proxy route management |
| `/api/acme` | ACME certificate management |
| `/api/applications` | Container apps, agent updates |
| `/api/containers` | nspawn container lifecycle |
| `/api/hosts` | Multi-host management, WoL, energy |
| `/api/cloud-relay` | Cloud relay control |
| `/api/dataverse` | Data engine (schema, tables, rows) |
| `/api/store` | App store catalog and releases |
| `/api/updates` | System update management |
| `/api/ws` | WebSocket connections |
| `/api/health` | Health check |

## Project Structure

```
homeroute/
├── crates/                 # Rust workspace (17 crates)
│   ├── homeroute/          # Main binary
│   └── hr-*/               # Service crates
├── web/                    # React/Vite dashboard
│   └── src/
│       ├── pages/          # Dashboard, DNS, Adblock, ReverseProxy, ...
│       ├── components/     # Layout, Sidebar, shared components
│       ├── context/        # AuthContext
│       └── api/            # API client (Axios)
├── store/                  # Expo Android app store client
│   └── src/
│       ├── screens/        # Catalog, AppDetail, Settings
│       └── api/            # API client
├── Makefile                # Build system
├── .env                    # Environment configuration
└── data/                   # Runtime data (auth.db, users.yml, hosts.json)
```

## License

Private project.

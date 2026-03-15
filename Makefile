# HomeRoute Build System
# Usage: make all, make deploy, make test
# DEV server: cloudmaster (10.0.0.10) — build only
# PROD server: 10.0.0.254 — runs homeroute

PROD_HOST := romain@10.0.0.20
PROD_DIR  := /opt/homeroute
PROD_API  := http://10.0.0.20:4000

.PHONY: server netcore edge orchestrator web studio all deploy deploy-prod deploy-netcore deploy-edge deploy-orchestrator test clean store agent agent-prod host-agent host-agent-prod check-prod check-not-prod

SHELL := /bin/bash

# Safety: block local deploy on dev server
check-not-prod:
	@if systemctl is-active --quiet homeroute 2>/dev/null; then \
		echo "OK: homeroute is running locally, assuming production server."; \
	else \
		echo "⛔ homeroute is NOT running locally. Use 'make deploy-prod' to deploy to production." && exit 1; \
	fi

# Safety: verify prod is reachable
check-prod:
	@echo "Checking production server..."
	@ssh -o ConnectTimeout=5 -o BatchMode=yes $(PROD_HOST) 'sudo systemctl is-active homeroute' > /dev/null 2>&1 \
		|| (echo "⛔ Cannot reach production server or homeroute is not running" && exit 1)
	@echo "✓ Production server OK"

# Build hr-netcore binary (DNS/DHCP/Adblock/IPv6)
netcore:
	cd crates && cargo build --release -p hr-netcore

# Build hr-edge binary (Proxy/TLS/ACME/Auth/Tunnel)
edge:
	cd crates && cargo build --release -p hr-edge

# Build hr-orchestrator binary (Containers/Registry/Git)
orchestrator:
	cd crates && cargo build --release -p hr-orchestrator

# Build server binary (API/Proxy/Auth/etc.)
server:
	cd crates && cargo build --release -p homeroute

# Build Vite React frontend
web:
	cd web && npm run build

# Build Studio frontend (Claude Code headless UI)
studio:
	cd web-studio && npm install --silent && npm run build

# Full build (studio + netcore + edge + orchestrator + server + frontend)
all: studio netcore edge orchestrator server web

# Deploy locally (only works on prod server itself)
deploy: check-not-prod all
	cp systemd/*.service /etc/systemd/system/ && systemctl daemon-reload
	systemctl restart hr-edge
	systemctl restart hr-orchestrator
	systemctl restart homeroute

# Deploy to production from dev server (restarts hr-edge + hr-orchestrator + homeroute, NOT hr-netcore)
deploy-prod: check-prod all
	@echo "Deploying to production ($(PROD_HOST))..."
	rsync -az --info=progress2 crates/target/release/homeroute $(PROD_HOST):$(PROD_DIR)/crates/target/release/homeroute
	rsync -az --info=progress2 crates/target/release/hr-edge $(PROD_HOST):$(PROD_DIR)/crates/target/release/hr-edge
	rsync -az --info=progress2 crates/target/release/hr-orchestrator $(PROD_HOST):$(PROD_DIR)/crates/target/release/hr-orchestrator
	rsync -az --info=progress2 crates/target/release/hr-netcore $(PROD_HOST):$(PROD_DIR)/crates/target/release/hr-netcore
	rsync -az --delete web/dist/ $(PROD_HOST):$(PROD_DIR)/web/dist/
	rsync -az --delete web-studio/dist/ $(PROD_HOST):$(PROD_DIR)/web-studio/dist/
	rsync -az systemd/ $(PROD_HOST):$(PROD_DIR)/systemd/
	ssh $(PROD_HOST) 'sudo cp $(PROD_DIR)/systemd/*.service /etc/systemd/system/ && sudo systemctl daemon-reload'
	ssh $(PROD_HOST) 'sudo systemctl restart hr-edge && sudo systemctl restart hr-orchestrator && sudo systemctl restart homeroute'
	@sleep 3
	@curl -sf $(PROD_API)/api/health | python3 -m json.tool \
		&& echo "✓ Deploy OK" \
		|| (echo "⛔ Health check FAILED — check logs: ssh $(PROD_HOST) 'journalctl -u homeroute -u hr-edge -u hr-orchestrator -n 50'" && exit 1)

# Deploy hr-edge separately (Proxy/TLS/ACME/Auth/Tunnel)
deploy-edge: check-prod edge
	@echo "Deploying hr-edge to production..."
	rsync -az --info=progress2 crates/target/release/hr-edge $(PROD_HOST):$(PROD_DIR)/crates/target/release/hr-edge
	ssh $(PROD_HOST) 'sudo systemctl restart hr-edge'
	@sleep 2
	@echo "✓ hr-edge deployed"

# Deploy hr-orchestrator separately (Containers/Registry/Git)
deploy-orchestrator: check-prod orchestrator
	@echo "Deploying hr-orchestrator to production..."
	rsync -az --info=progress2 crates/target/release/hr-orchestrator $(PROD_HOST):$(PROD_DIR)/crates/target/release/hr-orchestrator
	ssh $(PROD_HOST) 'sudo systemctl restart hr-orchestrator'
	@sleep 2
	@echo "✓ hr-orchestrator deployed"

# Deploy hr-netcore separately (rare — only when DNS/DHCP/Adblock/IPv6 code changes)
deploy-netcore: check-prod netcore
	@echo "Deploying hr-netcore to production..."
	rsync -az --info=progress2 crates/target/release/hr-netcore $(PROD_HOST):$(PROD_DIR)/crates/target/release/hr-netcore
	ssh $(PROD_HOST) 'sudo systemctl restart hr-netcore'
	@sleep 1
	@echo "✓ hr-netcore deployed"

# Run tests
test:
	cd crates && cargo test

# Build hr-agent binary (auto-increments version)
agent:
	@CURRENT=$$(grep '^version' crates/agents/hr-agent/Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/') && \
	MAJOR=$$(echo "$$CURRENT" | cut -d. -f1) && \
	MINOR=$$(echo "$$CURRENT" | cut -d. -f2) && \
	PATCH=$$(echo "$$CURRENT" | cut -d. -f3) && \
	NEW_PATCH=$$((PATCH + 1)) && \
	NEW_VERSION="$$MAJOR.$$MINOR.$$NEW_PATCH" && \
	sed -i "s/^version = \"$$CURRENT\"/version = \"$$NEW_VERSION\"/" crates/agents/hr-agent/Cargo.toml && \
	echo "Building hr-agent v$$NEW_VERSION..." && \
	cd crates && cargo build --release -p hr-agent && cd .. && \
	cp crates/target/release/hr-agent data/agent-binaries/hr-agent && \
	echo "$$NEW_VERSION" > data/agent-binaries/hr-agent.version && \
	echo "hr-agent v$$NEW_VERSION → data/agent-binaries/" && \
	echo "Run: make agent-prod   (to push to production containers)"

# Deploy hr-agent to production containers
agent-prod: check-prod
	@echo "Pushing hr-agent to production..."
	rsync -az data/agent-binaries/ $(PROD_HOST):$(PROD_DIR)/data/agent-binaries/
	ssh $(PROD_HOST) 'curl -sf -X POST http://127.0.0.1:4000/api/applications/agents/update' \
		&& echo "✓ Agent update triggered on production" \
		|| echo "⛔ Agent update failed"

# Build hr-host-agent binary (auto-increments version)
host-agent:
	@CURRENT=$$(grep '^version' crates/agents/hr-host-agent/Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/') && \
	MAJOR=$$(echo "$$CURRENT" | cut -d. -f1) && \
	MINOR=$$(echo "$$CURRENT" | cut -d. -f2) && \
	PATCH=$$(echo "$$CURRENT" | cut -d. -f3) && \
	NEW_PATCH=$$((PATCH + 1)) && \
	NEW_VERSION="$$MAJOR.$$MINOR.$$NEW_PATCH" && \
	sed -i "s/^version = \"$$CURRENT\"/version = \"$$NEW_VERSION\"/" crates/agents/hr-host-agent/Cargo.toml && \
	echo "Building hr-host-agent v$$NEW_VERSION..." && \
	cd crates && cargo build --release -p hr-host-agent

# Deploy hr-host-agent to local host (CloudMaster) + restart
host-agent-prod:
	@VERSION=$$(grep '^version' crates/agents/hr-host-agent/Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/') && \
	echo "Deploying hr-host-agent v$$VERSION..." && \
	systemctl stop hr-host-agent && \
	cp crates/target/release/hr-host-agent /usr/local/bin/hr-host-agent && \
	systemctl start hr-host-agent && \
	sleep 1 && \
	systemctl is-active --quiet hr-host-agent && \
	echo "✓ hr-host-agent v$$VERSION deployed and running" || \
	(echo "⛔ hr-host-agent failed to start" && journalctl -u hr-host-agent -n 10 --no-pager && exit 1)

# Build Flutter store APK (auto-increments versionCode)
store:
	@cd store_flutter && \
	CURRENT_CODE=$$(grep 'versionCode' android/app/build.gradle.kts | sed 's/[^0-9]//g') && \
	NEW_CODE=$$((CURRENT_CODE + 1)) && \
	CURRENT_NAME=$$(grep 'versionName' android/app/build.gradle.kts | sed 's/.*"\(.*\)".*/\1/') && \
	MAJOR=$$(echo "$$CURRENT_NAME" | cut -d. -f1) && \
	MINOR=$$(echo "$$CURRENT_NAME" | cut -d. -f2) && \
	NEW_NAME="$$MAJOR.$$MINOR.$$NEW_CODE" && \
	sed -i "s/versionCode = $$CURRENT_CODE/versionCode = $$NEW_CODE/" android/app/build.gradle.kts && \
	sed -i "s/versionName = \"$$CURRENT_NAME\"/versionName = \"$$NEW_NAME\"/" android/app/build.gradle.kts && \
	sed -i "s/^version: .*/version: $$NEW_NAME+$$NEW_CODE/" pubspec.yaml && \
	echo "Building store v$$NEW_NAME (code $$NEW_CODE)..." && \
	flutter build apk --release && \
	cp build/app/outputs/flutter-apk/app-release.apk /opt/homeroute/data/store/client/homeroute-store.apk && \
	APK_SIZE=$$(stat -c%s /opt/homeroute/data/store/client/homeroute-store.apk) && \
	echo "{\"version\":\"$$NEW_NAME\",\"changelog\":\"\",\"size_bytes\":$$APK_SIZE}" > /opt/homeroute/data/store/client/version.json && \
	echo "Deployed store v$$NEW_NAME → /api/store/client/apk"

# Clean build artifacts
clean:
	cd crates && cargo clean
	rm -rf web/dist

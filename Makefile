# HomeRoute Build System
# Usage: make all, make deploy, make test
# DEV server: cloudmaster (10.0.0.10) — build only
# PROD server: 10.0.0.254 — runs homeroute

PROD_HOST := romain@10.0.0.20
PROD_DIR  := /opt/homeroute
PROD_API  := http://10.0.0.20:4000

.PHONY: server netcore edge orchestrator web all deploy deploy-prod deploy-netcore deploy-edge deploy-orchestrator deploy-studio test clean store host-agent host-agent-prod check-prod check-not-prod check-on-cloudmaster

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
	@ssh -o ConnectTimeout=5 -o BatchMode=yes $(PROD_HOST) 'sudo -n systemctl is-active homeroute' > /dev/null 2>&1 \
		|| (echo "⛔ Cannot reach production server or homeroute is not running" && exit 1)
	@echo "✓ Production server OK"

# Safety: ensure builds happen on CloudMaster (10.0.0.10), not on prod or random hosts.
# Override with FORCE_BUILD=1 if you really know what you're doing.
check-on-cloudmaster:
	@if [ "$$FORCE_BUILD" = "1" ]; then \
		echo "⚠ FORCE_BUILD=1 — skipping host check (you are on $$(hostname))"; \
	elif [ "$$(hostname)" != "cloudmaster" ]; then \
		echo "⛔ Builds must run on cloudmaster (10.0.0.10), not on $$(hostname)."; \
		echo "   SSH there: ssh romain@10.0.0.10  (then cd /nvme/homeroute && make ...)"; \
		echo "   Override (rare): make ... FORCE_BUILD=1"; \
		exit 1; \
	fi

# Build hr-netcore binary (DNS/DHCP/Adblock/IPv6)
netcore:
	cd crates && cargo build --release -p hr-netcore

# Build hr-edge binary (Proxy/TLS/ACME/Auth/Tunnel)
edge:
	cd crates && cargo build --release -p hr-edge

# Build hr-orchestrator binary (hr-apps supervisor, Git, DB)
orchestrator:
	cd crates && cargo build --release -p hr-orchestrator

# Build server binary (API/Proxy/Auth/etc.)
server:
	cd crates && cargo build --release -p homeroute

# Build Vite React frontend
web:
	cd web && npm run build

# Full build (netcore + edge + orchestrator + server + frontend)
all: netcore edge orchestrator server web

# Deploy locally (only works on prod server itself)
deploy: check-not-prod all
	cp systemd/*.service /etc/systemd/system/ && systemctl daemon-reload
	systemctl restart hr-edge
	systemctl restart hr-orchestrator
	systemctl restart homeroute

# Deploy to production from dev server (restarts hr-edge + hr-orchestrator + homeroute, NOT hr-netcore)
deploy-prod: check-on-cloudmaster check-prod all
	@echo "Deploying to production ($(PROD_HOST))..."
	rsync -az --info=progress2 crates/target/release/homeroute $(PROD_HOST):$(PROD_DIR)/crates/target/release/homeroute
	rsync -az --info=progress2 crates/target/release/hr-edge $(PROD_HOST):$(PROD_DIR)/crates/target/release/hr-edge
	rsync -az --info=progress2 crates/target/release/hr-orchestrator $(PROD_HOST):$(PROD_DIR)/crates/target/release/hr-orchestrator
	rsync -az --info=progress2 crates/target/release/hr-netcore $(PROD_HOST):$(PROD_DIR)/crates/target/release/hr-netcore
	rsync -az --delete web/dist/ $(PROD_HOST):$(PROD_DIR)/web/dist/
	rsync -az systemd/ $(PROD_HOST):$(PROD_DIR)/systemd/
	ssh $(PROD_HOST) 'sudo cp $(PROD_DIR)/systemd/*.service /etc/systemd/system/ && sudo systemctl daemon-reload'
	ssh $(PROD_HOST) 'sudo systemctl restart hr-edge && sudo systemctl restart hr-orchestrator && sudo systemctl restart homeroute'
	@sleep 3
	@curl -sf $(PROD_API)/api/health | python3 -m json.tool \
		&& echo "✓ Deploy OK" \
		|| (echo "⛔ Health check FAILED — check logs: ssh $(PROD_HOST) 'journalctl -u homeroute -u hr-edge -u hr-orchestrator -n 50'" && exit 1)

# Deploy hr-edge separately (Proxy/TLS/ACME/Auth/Tunnel)
deploy-edge: check-on-cloudmaster check-prod edge
	@echo "Deploying hr-edge to production..."
	rsync -az --info=progress2 crates/target/release/hr-edge $(PROD_HOST):$(PROD_DIR)/crates/target/release/hr-edge
	ssh $(PROD_HOST) 'sudo systemctl restart hr-edge'
	@sleep 2
	@echo "✓ hr-edge deployed"

# Deploy hr-orchestrator separately (hr-apps supervisor, Git, DB)
deploy-orchestrator: check-on-cloudmaster check-prod orchestrator
	@echo "Deploying hr-orchestrator to production..."
	rsync -az --info=progress2 crates/target/release/hr-orchestrator $(PROD_HOST):$(PROD_DIR)/crates/target/release/hr-orchestrator
	rsync -az systemd/hr-apps.slice $(PROD_HOST):$(PROD_DIR)/systemd/hr-apps.slice
	ssh $(PROD_HOST) 'sudo cp $(PROD_DIR)/systemd/hr-apps.slice /etc/systemd/system/hr-apps.slice && sudo systemctl daemon-reload'
	ssh $(PROD_HOST) 'sudo systemctl restart hr-orchestrator'
	@sleep 2
	@echo "✓ hr-orchestrator deployed"

# Deploy hr-netcore separately (rare — only when DNS/DHCP/Adblock/IPv6 code changes)
deploy-netcore: check-on-cloudmaster check-prod netcore
	@echo "Deploying hr-netcore to production..."
	rsync -az --info=progress2 crates/target/release/hr-netcore $(PROD_HOST):$(PROD_DIR)/crates/target/release/hr-netcore
	ssh $(PROD_HOST) 'sudo systemctl restart hr-netcore'
	@sleep 1
	@echo "✓ hr-netcore deployed"

# Install / refresh code-server on the production router (studio.mynetwk.biz → 127.0.0.1:8443)
deploy-studio: check-on-cloudmaster check-prod
	@echo "Installing/refreshing code-server on production..."
	rsync -az scripts/setup-studio.sh $(PROD_HOST):$(PROD_DIR)/scripts/setup-studio.sh
	ssh $(PROD_HOST) 'sudo bash $(PROD_DIR)/scripts/setup-studio.sh'
	@echo "✓ Studio (code-server) ready on studio.mynetwk.biz"

# Run tests
test:
	cd crates && cargo test

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

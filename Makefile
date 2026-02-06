# HomeRoute Build System
# Usage: make all, make deploy, make test

.PHONY: server web all deploy test clean

# Build server binary
server:
	cd crates && cargo build --release

# Build Vite React frontend
web:
	cd web && npm run build

# Full build (server + frontend)
all: server web

# Deploy (build + restart service)
deploy: all
	systemctl restart homeroute

# Run tests
test:
	cd crates && cargo test

# Clean build artifacts
clean:
	cd crates && cargo clean
	rm -rf web/dist

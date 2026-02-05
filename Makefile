# HomeRoute Build System
# Usage: make all, make deploy, make test

WASM_TARGET = wasm32-unknown-unknown
SITE_DIR    = target/site
PKG_DIR     = $(SITE_DIR)/pkg
PUBLIC_DIR  = crates/hr-web-client/public

.PHONY: server wasm css assets web all deploy test clean

# Build server binary (native, includes SSR)
server:
	cd crates && cargo build --release

# Build WASM (islands hydration only)
wasm:
	cd crates && cargo build --release --target $(WASM_TARGET) -p hr-web-client
	mkdir -p $(PKG_DIR)
	wasm-bindgen crates/target/$(WASM_TARGET)/release/hr_web_client.wasm \
		--out-dir $(PKG_DIR) --target web --no-typescript

# Tailwind CSS
css:
	npx tailwindcss \
		-i crates/hr-web/style/input.css \
		-o $(PKG_DIR)/style.css \
		--minify

# Copy static assets (favicon, xterm-bridge.js, etc.)
assets:
	mkdir -p $(SITE_DIR)
	@if [ -d "$(PUBLIC_DIR)" ] && [ "$$(ls -A $(PUBLIC_DIR) 2>/dev/null)" ]; then \
		cp -r $(PUBLIC_DIR)/* $(SITE_DIR)/; \
	fi

# Full frontend build
web: wasm css assets

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
	rm -rf $(SITE_DIR)

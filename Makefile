.PHONY: help install build deploy clean test \
       worker-build worker-deploy worker-dev worker-clean \
       client-install client-build client-pack client-deploy client-clean \
       example-install example-build example-dev example-deploy example-clean \
       test-worker test-client

# ─── Config ──────────────────────────────────────────────
-include .env

WORKER_DIR    = worker
CLIENT_DIR    = client
EXAMPLE_DIR   = example
PAGES_PROJECT ?= zrtc-demo
WORKER_DOMAIN ?= your-worker.workers.dev

# ─── Help ────────────────────────────────────────────────
help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | \
	  awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2}'

# ─── All ─────────────────────────────────────────────────
install: client-install example-install ## Install all JS deps
build: worker-build client-build example-build ## Build everything
deploy: worker-deploy client-deploy example-deploy ## Deploy everything
clean: worker-clean client-clean example-clean ## Clean all build artifacts
test: test-client test-worker ## Run all tests

# ─── Worker (Rust → Cloudflare Workers) ──────────────────
worker-build: ## Build worker (Rust → WASM)
	cd $(WORKER_DIR) && cargo install worker-build 2>/dev/null; worker-build --release

worker-dev: ## Run worker locally (wrangler dev)
	cd $(WORKER_DIR) && npx wrangler dev

worker-deploy: worker-build ## Deploy worker to Cloudflare
	cd $(WORKER_DIR) && CLOUDFLARE_ACCOUNT_ID=$(CLOUDFLARE_ACCOUNT_ID) npx wrangler deploy

worker-clean: ## Clean worker build artifacts
	cd $(WORKER_DIR) && cargo clean && rm -rf build/

# ─── Client (JS library → npm) ──────────────────────────
client-install: ## Install client dependencies
	cd $(CLIENT_DIR) && npm install

client-build: client-install ## Build client library
	cd $(CLIENT_DIR) && npm run build

client-pack: client-build ## Dry-run npm pack (check what gets published)
	cd $(CLIENT_DIR) && npm pack --dry-run

client-deploy: ## Bump patch version, commit, tag, push (triggers GitHub Actions)
	@cd $(CLIENT_DIR) && npm version patch --no-git-tag-version; \
	VERSION=$$(node -p "require('./package.json').version"); \
	TAG="v$$VERSION"; \
	cd .. && git add $(CLIENT_DIR)/package.json && git commit -m "release: $$TAG" && \
	git tag "$$TAG" && git push origin HEAD "$$TAG"; \
	echo "Pushed tag $$TAG — GitHub Actions will publish to npm."

client-clean: ## Clean client build artifacts
	rm -rf $(CLIENT_DIR)/dist $(CLIENT_DIR)/node_modules

# ─── Example (Vue 3 + Vite → Cloudflare Pages) ──────────
example-install: ## Install example dependencies
	cd $(EXAMPLE_DIR) && npm install

example-build: example-install ## Build example for production
	cd $(EXAMPLE_DIR) && VITE_WORKER_URL=https://$(WORKER_DOMAIN) npm run build

example-dev: example-install ## Run example dev server
	cd $(EXAMPLE_DIR) && VITE_WORKER_URL=https://$(WORKER_DOMAIN) npm run dev

example-deploy: example-build ## Deploy example to Cloudflare Pages
	cd $(EXAMPLE_DIR) && CLOUDFLARE_ACCOUNT_ID=$(CLOUDFLARE_ACCOUNT_ID) npx wrangler pages deploy dist/ --project-name=$(PAGES_PROJECT) --branch=main

example-clean: ## Clean example build artifacts
	rm -rf $(EXAMPLE_DIR)/dist $(EXAMPLE_DIR)/node_modules

# ─── Tests ───────────────────────────────────────────────
WORKER_URL ?= https://$(WORKER_DOMAIN)

test-worker: ## Integration-test worker API (live or WORKER_URL=http://localhost:8787)
	@bash tests/test-worker-api.sh $(WORKER_URL)

test-client: ## Unit-test client chunking logic
	@node tests/test-chunking.js

# ─── First-time setup ───────────────────────────────────
setup: ## First-time project setup (install deps, create R2 bucket)
	@echo "── Installing client deps ──"
	cd $(CLIENT_DIR) && npm install
	@echo "── Installing example deps ──"
	cd $(EXAMPLE_DIR) && npm install
	@echo "── Creating R2 bucket (idempotent) ──"
	-npx wrangler r2 bucket create zrtc 2>/dev/null || true
	@echo ""
	@echo "✓ Setup complete. Next steps:"
	@echo "  make worker-dev     # Run worker locally"
	@echo "  make example-dev    # Run example locally"
	@echo "  make deploy         # Deploy everything"

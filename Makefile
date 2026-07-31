# ┌──────────────────────────────────────────────────────────────────────────────┐
# │ MCP-Shield Makefile                                                          │
# │                                                                              │
# │ Common development, testing, building, and deployment targets.              │
# └──────────────────────────────────────────────────────────────────────────────┘

# ─────────────────────────────────────────────────────────────────────────────
# Configuration
# ─────────────────────────────────────────────────────────────────────────────
SHELL := /bin/bash
.DEFAULT_GOAL := help

# Project metadata
PROJECT_NAME := mcp-shield
PROJECT_VERSION := $(shell cargo metadata --format-version=1 2>/dev/null | jq -r '.packages[] | select(.name=="mcp-shield") | .version' 2>/dev/null || echo "0.1.0")
GIT_COMMIT := $(shell git rev-parse --short HEAD 2>/dev/null || echo "unknown")
GIT_TAG := $(shell git describe --tags --abbrev=0 2>/dev/null || echo "v0.1.0")
BUILD_DATE := $(shell date -u +"%Y-%m-%dT%H:%M:%SZ")
RUST_VERSION := 1.97

# Docker configuration
DOCKER_REGISTRY ?= ghcr.io
DOCKER_NAMESPACE ?= $(shell git config --get remote.origin.url | sed -E 's/.*github.com[:/](.+)\.git/\1/' | tr '[:upper:]' '[:lower:]' || echo "mcp-shield")
DOCKER_IMAGE := $(DOCKER_REGISTRY)/$(DOCKER_NAMESPACE)/$(PROJECT_NAME)
DOCKERFILE := docker/Dockerfile

# Kubernetes/Helm configuration
HELM_CHART_DIR := helm/mcp-shield
HELM_RELEASE_NAME ?= mcp-shield
HELM_NAMESPACE ?= mcp-shield
KUBECONFIG ?= ~/.kube/config

# Colors
RED := \033[0;31m
GREEN := \033[0;32m
YELLOW := \033[1;33m
BLUE := \033[0;34m
NC := \033[0m

# ─────────────────────────────────────────────────────────────────────────────
# Help
# ─────────────────────────────────────────────────────────────────────────────
.PHONY: help
help: ## Show this help message
	@echo "$(BLUE)$(PROJECT_NAME) - Layer 7 MCP Security Gateway$(NC)"
	@echo ""
	@echo "$(GREEN)Usage:$(NC) make [target]"
	@echo ""
	@awk 'BEGIN {FS = ":.*##"; printf "\n"} /^[a-zA-Z_-]+:.*?##/ { printf "  $(YELLOW)%-30s$(NC) %s\n", $$1, $$2 }' $(MAKEFILE_LIST)
	@echo ""

# ─────────────────────────────────────────────────────────────────────────────
# Development
# ─────────────────────────────────────────────────────────────────────────────
.PHONY: dev-setup
dev-setup: ## Set up development environment
	@echo "$(BLUE)Setting up development environment...$(NC)"
	rustup toolchain install $(RUST_VERSION) --component rustfmt,clippy
	cargo install cargo-watch cargo-audit cargo-deny cargo-outdated cargo-udeps cargo-nextest cargo-llvm-cov
	@echo "$(GREEN)Development environment ready!$(NC)"

.PHONY: dev
dev: ## Run in development mode with hot reload
	@echo "$(BLUE)Starting development server...$(NC)"
	cargo watch -x "run -- --config config/default.toml"

.PHONY: dev-docker
dev-docker: ## Start full development stack with Docker Compose
	@echo "$(BLUE)Starting development stack...$(NC)"
	docker-compose -f docker-compose.yml -f docker-compose.dev.yml up -d

.PHONY: dev-docker-down
dev-docker-down: ## Stop development stack
	@echo "$(BLUE)Stopping development stack...$(NC)"
	docker-compose -f docker-compose.yml -f docker-compose.dev.yml down -v

.PHONY: dev-docker-logs
dev-docker-logs: ## View development stack logs
	docker-compose -f docker-compose.yml -f docker-compose.dev.yml logs -f

.PHONY: dev-docker-shell
dev-docker-shell: ## Open shell in development container
	docker-compose -f docker-compose.yml -f docker-compose.dev.yml exec mcp-shield bash

# ─────────────────────────────────────────────────────────────────────────────
# Build
# ─────────────────────────────────────────────────────────────────────────────
.PHONY: build
build: ## Build release binary
	@echo "$(BLUE)Building release binary...$(NC)"
	cargo build --release --locked

.PHONY: build-all
build-all: ## Build all targets
	@echo "$(BLUE)Building all targets...$(NC)"
	cargo build --release --locked --all-targets

.PHONY: build-dev
build-dev: ## Build debug binary
	@echo "$(BLUE)Building debug binary...$(NC)"
	cargo build --locked

.PHONY: check
check: ## Quick compile check
	@echo "$(BLUE)Checking code...$(NC)"
	cargo check --locked --all-targets

.PHONY: clean
clean: ## Clean build artifacts
	@echo "$(BLUE)Cleaning build artifacts...$(NC)"
	cargo clean

# ─────────────────────────────────────────────────────────────────────────────
# Testing
# ─────────────────────────────────────────────────────────────────────────────
.PHONY: test
test: ## Run all tests
	@echo "$(BLUE)Running all tests...$(NC)"
	cargo test --locked --all

.PHONY: test-unit
test-unit: ## Run unit tests only
	@echo "$(BLUE)Running unit tests...$(NC)"
	cargo test --locked --lib

.PHONY: test-integration
test-integration: ## Run integration tests
	@echo "$(BLUE)Running integration tests...$(NC)"
	cargo test --locked --test integration

.PHONY: test-e2e
test-e2e: ## Run end-to-end tests
	@echo "$(BLUE)Running E2E tests...$(NC)"
	cargo test --locked --test e2e

.PHONY: test-nextest
test-nextest: ## Run tests with nextest (faster)
	@echo "$(BLUE)Running tests with nextest...$(NC)"
	cargo nextest run --locked --all

.PHONY: test-coverage
test-coverage: ## Generate test coverage report
	@echo "$(BLUE)Generating coverage report...$(NC)"
	cargo llvm-cov --locked --all --workspace --lcov --output-path coverage.lcov
	genhtml coverage.lcov --output-directory coverage-report
	@echo "$(GREEN)Coverage report generated in coverage-report/$(NC)"

.PHONY: bench
bench: ## Run benchmarks
	@echo "$(BLUE)Running benchmarks...$(NC)"
	cargo bench --locked --all

# ─────────────────────────────────────────────────────────────────────────────
# Code Quality
# ─────────────────────────────────────────────────────────────────────────────
.PHONY: fmt
fmt: ## Format code with rustfmt
	@echo "$(BLUE)Formatting code...$(NC)"
	cargo fmt --all

.PHONY: fmt-check
fmt-check: ## Check formatting without changes
	@echo "$(BLUE)Checking formatting...$(NC)"
	cargo fmt --all -- --check

.PHONY: clippy
clippy: ## Run clippy lints
	@echo "$(BLUE)Running clippy...$(NC)"
	cargo clippy --locked --all-targets --all-features -- -D warnings

.PHONY: audit
audit: ## Audit dependencies for vulnerabilities
	@echo "$(BLUE)Auditing dependencies...$(NC)"
	cargo audit

.PHONY: deny
deny: ## Run cargo-deny (licenses, bans, advisories, sources)
	@echo "$(BLUE)Running cargo-deny...$(NC)"
	cargo deny check

.PHONY: outdated
outdated: ## Check for outdated dependencies
	@echo "$(BLUE)Checking for outdated dependencies...$(NC)"
	cargo outdated -R --exit-code 1

.PHONY: udeps
udeps: ## Check for unused dependencies
	@echo "$(BLUE)Checking for unused dependencies...$(NC)"
	cargo +nightly udeps --all-targets

# ─────────────────────────────────────────────────────────────────────────────
# Docker
# ─────────────────────────────────────────────────────────────────────────────
.PHONY: docker-build
docker-build: ## Build Docker image (multi-arch)
	@echo "$(BLUE)Building Docker image...$(NC)"
	docker buildx build \
		--platform linux/amd64,linux/arm64 \
		--tag $(DOCKER_IMAGE):$(PROJECT_VERSION) \
		--tag $(DOCKER_IMAGE):latest \
		--file $(DOCKERFILE) \
		--build-arg BUILD_DATE=$(BUILD_DATE) \
		--build-arg BUILD_VERSION=$(PROJECT_VERSION) \
		--build-arg VCS_REF=$(GIT_COMMIT) \
		--build-arg VCS_URL=$(shell git config --get remote.origin.url) \
		--load \
		.

.PHONY: docker-build-local
docker-build-local: ## Build Docker image for local arch only
	@echo "$(BLUE)Building Docker image (local)...$(NC)"
	docker build \
		--tag $(DOCKER_IMAGE):$(PROJECT_VERSION) \
		--tag $(DOCKER_IMAGE):latest \
		--file $(DOCKERFILE) \
		--build-arg BUILD_DATE=$(BUILD_DATE) \
		--build-arg BUILD_VERSION=$(PROJECT_VERSION) \
		--build-arg VCS_REF=$(GIT_COMMIT) \
		--build-arg VCS_URL=$(shell git config --get remote.origin.url) \
		.

.PHONY: docker-push
docker-push: ## Push Docker image to registry
	@echo "$(BLUE)Pushing Docker image...$(NC)"
	docker buildx build \
		--platform linux/amd64,linux/arm64 \
		--tag $(DOCKER_IMAGE):$(PROJECT_VERSION) \
		--tag $(DOCKER_IMAGE):latest \
		--file $(DOCKERFILE) \
		--build-arg BUILD_DATE=$(BUILD_DATE) \
		--build-arg BUILD_VERSION=$(PROJECT_VERSION) \
		--build-arg VCS_REF=$(GIT_COMMIT) \
		--build-arg VCS_URL=$(shell git config --get remote.origin.url) \
		--push \
		.

.PHONY: docker-scan
docker-scan: ## Scan Docker image for vulnerabilities
	@echo "$(BLUE)Scanning Docker image...$(NC)"
	trivy image --severity HIGH,CRITICAL $(DOCKER_IMAGE):$(PROJECT_VERSION)
	grype $(DOCKER_IMAGE):$(PROJECT_VERSION) --fail-on high

.PHONY: docker-run
docker-run: ## Run Docker container locally
	@echo "$(BLUE)Running Docker container...$(NC)"
	docker run --rm -it \
		-p 8080:8080 \
		-p 9090:9090 \
		-p 9091:9091 \
		-v $(PWD)/config:/etc/mcp-shield/config:ro \
		-v $(PWD)/policies:/etc/mcp-shield/policies:ro \
		$(DOCKER_IMAGE):$(PROJECT_VERSION)

.PHONY: docker-shell
docker-shell: ## Open shell in Docker container
	docker run --rm -it \
		--entrypoint /bin/bash \
		-v $(PWD)/config:/etc/mcp-shield/config:ro \
		-v $(PWD)/policies:/etc/mcp-shield/policies:ro \
		$(DOCKER_IMAGE):$(PROJECT_VERSION)

# ─────────────────────────────────────────────────────────────────────────────
# Docker Compose
# ─────────────────────────────────────────────────────────────────────────────
.PHONY: compose-up
compose-up: ## Start full stack with Docker Compose
	@echo "$(BLUE)Starting full stack...$(NC)"
	docker-compose -f docker-compose.yml up -d

.PHONY: compose-down
compose-down: ## Stop full stack
	@echo "$(BLUE)Stopping full stack...$(NC)"
	docker-compose -f docker-compose.yml down -v

.PHONY: compose-logs
compose-logs: ## View full stack logs
	docker-compose -f docker-compose.yml logs -f

.PHONY: compose-ps
compose-ps: ## List running services
	docker-compose -f docker-compose.yml ps

.PHONY: compose-restart
compose-restart: ## Restart all services
	docker-compose -f docker-compose.yml restart

# ─────────────────────────────────────────────────────────────────────────────
# Kubernetes / Helm
# ─────────────────────────────────────────────────────────────────────────────
.PHONY: helm-deps
helm-deps: ## Update Helm dependencies
	@echo "$(BLUE)Updating Helm dependencies...$(NC)"
	helm dependency update $(HELM_CHART_DIR)

.PHONY: helm-lint
helm-lint: ## Lint Helm chart
	@echo "$(BLUE)Linting Helm chart...$(NC)"
	helm lint $(HELM_CHART_DIR)

.PHONY: helm-template
helm-template: ## Template Helm chart
	@echo "$(BLUE)Templating Helm chart...$(NC)"
	helm template $(HELM_RELEASE_NAME) $(HELM_CHART_DIR) \
		--namespace $(HELM_NAMESPACE) \
		--create-namespace \
		--debug

.PHONY: helm-install
helm-install: ## Install Helm chart
	@echo "$(BLUE)Installing Helm chart...$(NC)"
	helm upgrade --install $(HELM_RELEASE_NAME) $(HELM_CHART_DIR) \
		--namespace $(HELM_NAMESPACE) \
		--create-namespace \
		--wait \
		--timeout 5m

.PHONY: helm-install-prod
helm-install-prod: ## Install Helm chart with production values
	@echo "$(BLUE)Installing Helm chart (production)...$(NC)"
	helm upgrade --install $(HELM_RELEASE_NAME) $(HELM_CHART_DIR) \
		--namespace $(HELM_NAMESPACE) \
		--create-namespace \
		--wait \
		--timeout 10m \
		-f $(HELM_CHART_DIR)/values-prod.yaml

.PHONY: helm-uninstall
helm-uninstall: ## Uninstall Helm chart
	@echo "$(BLUE)Uninstalling Helm chart...$(NC)"
	helm uninstall $(HELM_RELEASE_NAME) --namespace $(HELM_NAMESPACE)

.PHONY: helm-upgrade
helm-upgrade: ## Upgrade Helm chart
	@echo "$(BLUE)Upgrading Helm chart...$(NC)"
	helm upgrade $(HELM_RELEASE_NAME) $(HELM_CHART_DIR) \
		--namespace $(HELM_NAMESPACE) \
		--wait \
		--timeout 5m

.PHONY: helm-rollback
helm-rollback: ## Rollback Helm release
	@echo "$(BLUE)Rolling back Helm release...$(NC)"
	helm rollback $(HELM_RELEASE_NAME) --namespace $(HELM_NAMESPACE)

.PHONY: helm-history
helm-history: ## Show Helm release history
	helm history $(HELM_RELEASE_NAME) --namespace $(HELM_NAMESPACE)

.PHONY: helm-status
helm-status: ## Show Helm release status
	helm status $(HELM_RELEASE_NAME) --namespace $(HELM_NAMESPACE)

.PHONY: helm-values
helm-values: ## Show computed Helm values
	helm get values $(HELM_RELEASE_NAME) --namespace $(HELM_NAMESPACE) --all

.PHONY: helm-package
helm-package: ## Package Helm chart
	@echo "$(BLUE)Packaging Helm chart...$(NC)"
	helm package $(HELM_CHART_DIR) --destination /tmp/helm-charts

.PHONY: helm-push
helm-push: ## Push Helm chart to OCI registry
	@echo "$(BLUE)Pushing Helm chart to OCI registry...$(NC)"
	helm push $(HELM_CHART_DIR) oci://$(DOCKER_REGISTRY)/$(DOCKER_NAMESPACE)/charts

# ─────────────────────────────────────────────────────────────────────────────
# Kubernetes Operations
# ─────────────────────────────────────────────────────────────────────────────
.PHONY: k8s-apply
k8s-apply: ## Apply Kubernetes manifests
	@echo "$(BLUE)Applying Kubernetes manifests...$(NC)"
	kubectl apply -k k8s/overlays/production

.PHONY: k8s-delete
k8s-delete: ## Delete Kubernetes manifests
	@echo "$(BLUE)Deleting Kubernetes manifests...$(NC)"
	kubectl delete -k k8s/overlays/production

.PHONY: k8s-logs
k8s-logs: ## View pod logs
	kubectl logs -l app.kubernetes.io/name=$(PROJECT_NAME) -n $(HELM_NAMESPACE) -f --tail=100

.PHONY: k8s-shell
k8s-shell: ## Open shell in pod
	kubectl exec -it -n $(HELM_NAMESPACE) \
		$$(kubectl get pods -l app.kubernetes.io/name=$(PROJECT_NAME) -n $(HELM_NAMESPACE) -o jsonpath='{.items[0].metadata.name}') \
		-- /bin/bash

.PHONY: k8s-port-forward
k8s-port-forward: ## Port forward to gateway
	kubectl port-forward -n $(HELM_NAMESPACE) svc/$(HELM_RELEASE_NAME) 8080:8080 9090:9090 9091:9091

.PHONY: k8s-scale
k8s-scale: ## Scale deployment (usage: make k8s-scale REPLICAS=5)
	@echo "$(BLUE)Scaling deployment to $(REPLICAS) replicas...$(NC)"
	kubectl scale deployment $(HELM_RELEASE_NAME) --replicas=$(REPLICAS) -n $(HELM_NAMESPACE)

.PHONY: k8s-restart
k8s-restart: ## Restart deployment
	@echo "$(BLUE)Restarting deployment...$(NC)"
	kubectl rollout restart deployment $(HELM_RELEASE_NAME) -n $(HELM_NAMESPACE)

.PHONY: k8s-rollout-status
k8s-rollout-status: ## Check rollout status
	kubectl rollout status deployment $(HELM_RELEASE_NAME) -n $(HELM_NAMESPACE) --timeout=5m

# ─────────────────────────────────────────────────────────────────────────────
# Security
# ─────────────────────────────────────────────────────────────────────────────
.PHONY: security-scan
security-scan: ## Run all security scans
	@echo "$(BLUE)Running security scans...$(NC)"
	$(MAKE) audit
	$(MAKE) deny
	cargo audit --json > audit-report.json || true
	trivy fs --severity HIGH,CRITICAL --format json --output trivy-fs-report.json . || true

.PHONY: sbom
sbom: ## Generate SBOM
	@echo "$(BLUE)Generating SBOM...$(NC)"
	syft packages dir:. -o spdx-json=sbom.spdx.json
	syft packages dir:. -o cyclonedx-json=sbom.cyclonedx.json
	syft packages dir:. -o table=sbom.table.txt

.PHONY: sign
sign: ## Sign Docker image with Cosign
	@echo "$(BLUE)Signing Docker image...$(NC)"
	cosign sign --yes \
		--annotations "version=$(PROJECT_VERSION)" \
		--annotations "git-sha=$(GIT_COMMIT)" \
		--annotations "git-ref=$(GIT_TAG)" \
		$(DOCKER_IMAGE):$(PROJECT_VERSION)

.PHONY: verify
verify: ## Verify Docker image signature
	@echo "$(BLUE)Verifying Docker image signature...$(NC)"
	cosign verify \
		--certificate-identity-regexp=".*" \
		--certificate-oidc-issuer-regexp=".*" \
		$(DOCKER_IMAGE):$(PROJECT_VERSION)

# ─────────────────────────────────────────────────────────────────────────────
# Release
# ─────────────────────────────────────────────────────────────────────────────
.PHONY: release-dry-run
release-dry-run: ## Dry run semantic release
	@echo "$(BLUE)Running semantic release dry run...$(NC)"
	npx semantic-release --dry-run --no-ci

.PHONY: release
release: ## Create release (requires proper setup)
	@echo "$(BLUE)Creating release...$(NC)"
	npx semantic-release

# ─────────────────────────────────────────────────────────────────────────────
# Utility
# ─────────────────────────────────────────────────────────────────────────────
.PHONY: version
version: ## Show version information
	@echo "Project: $(PROJECT_NAME)"
	@echo "Version: $(PROJECT_VERSION)"
	@echo "Git Commit: $(GIT_COMMIT)"
	@echo "Git Tag: $(GIT_TAG)"
	@echo "Build Date: $(BUILD_DATE)"
	@echo "Rust Version: $(RUST_VERSION)"

.PHONY: deps-tree
deps-tree: ## Show dependency tree
	cargo tree --locked

.PHONY: deps-graph
deps-graph: ## Generate dependency graph (requires cargo-modules)
	cargo modules generate graph --output deps-graph.dot
	dot -Tpng deps-graph.dot -o deps-graph.png

.PHONY: doc
doc: ## Generate documentation
	@echo "$(BLUE)Generating documentation...$(NC)"
	cargo doc --locked --no-deps --open

.PHONY: update-deps
update-deps: ## Update dependencies
	@echo "$(BLUE)Updating dependencies...$(NC)"
	cargo update

.PHONY: install-tools
install-tools: ## Install development tools
	@echo "$(BLUE)Installing development tools...$(NC)"
	cargo install cargo-watch cargo-audit cargo-deny cargo-outdated cargo-udeps cargo-nextest cargo-llvm-cov cargo-modules

# ─────────────────────────────────────────────────────────────────────────────
# CI/CD Helpers
# ─────────────────────────────────────────────────────────────────────────────
.PHONY: ci-lint
ci-lint: fmt-check clippy audit deny ## Run all CI lint checks

.PHONY: ci-test
ci-test: test-unit test-integration ## Run CI tests

.PHONY: ci-build
ci-build: build docker-build-local ## Run CI build

.PHONY: ci-all
ci-all: ci-lint ci-test ci-build ## Run all CI checks locally

# ─────────────────────────────────────────────────────────────────────────────
# Cleanup
# ─────────────────────────────────────────────────────────────────────────────
.PHONY: clean-all
clean-all: clean ## Clean everything including Docker
	@echo "$(BLUE)Cleaning all artifacts...$(NC)"
	docker system prune -f
	rm -rf coverage-report sbom.* target

# ─────────────────────────────────────────────────────────────────────────────
# Phony targets
# ─────────────────────────────────────────────────────────────────────────────
.PHONY: help dev-setup dev dev-docker dev-docker-down dev-docker-logs dev-docker-shell \
        build build-all build-dev check clean \
        test test-unit test-integration test-e2e test-nextest test-coverage bench \
        fmt fmt-check clippy audit deny outdated udeps \
        docker-build docker-build-local docker-push docker-scan docker-run docker-shell \
        compose-up compose-down compose-logs compose-ps compose-restart \
        helm-deps helm-lint helm-template helm-install helm-install-prod helm-uninstall \
        helm-upgrade helm-rollback helm-history helm-status helm-values helm-package helm-push \
        k8s-apply k8s-delete k8s-logs k8s-shell k8s-port-forward k8s-scale k8s-restart k8s-rollout-status \
        security-scan sbom sign verify \
        release-dry-run release \
        version deps-tree deps-graph doc update-deps install-tools \
        ci-lint ci-test ci-build ci-all clean-all
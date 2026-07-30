#!/bin/bash
# ┌──────────────────────────────────────────────────────────────────────────────┐
# │ MCP-Shield Entrypoint Script                                                │
# │                                                                              │
# │ Handles:                                                                     │
# │   - Signal forwarding for graceful shutdown                                 │
# │   - Configuration validation and templating                                 │
# │   - Database migrations (Phase 4)                                           │
# │   - Secrets injection from files/env                                        │
# │   - Runtime user/group switching                                            │
# │   - Pre-start hooks                                                         │
# │   - Configuration templating from environment variables                     │
# └──────────────────────────────────────────────────────────────────────────────┘

set -euo pipefail

# ─────────────────────────────────────────────────────────────────────────────
# Configuration
# ─────────────────────────────────────────────────────────────────────────────
readonly SCRIPT_NAME="$(basename "$0")"
readonly MCP_SHIELD_USER="${MCP_SHIELD_USER:-nonroot}"
readonly MCP_SHIELD_UID="${MCP_SHIELD_UID:-65532}"
readonly MCP_SHIELD_GID="${MCP_SHIELD_GID:-65532}"
readonly MCP_SHIELD_CONFIG="${MCP_SHIELD_CONFIG:-/etc/mcp-shield/config/default.toml}"
readonly MCP_SHIELD_POLICIES_DIR="${MCP_SHIELD_POLICIES_DIR:-/etc/mcp-shield/policies}"
readonly MCP_SHIELD_DATA_DIR="${MCP_SHIELD_DATA_DIR:-/var/lib/mcp-shield/data}"
readonly MCP_SHIELD_LOG_DIR="${MCP_SHIELD_LOG_DIR:-/var/log/mcp-shield}"
readonly MCP_SHIELD_TMP_DIR="${MCP_SHIELD_TMP_DIR:-/tmp/mcp-shield}"
readonly MCP_SHIELD_PRE_START_HOOK="${MCP_SHIELD_PRE_START_HOOK:-/usr/local/bin/pre-start.sh}"
readonly MCP_SHIELD_POST_STOP_HOOK="${MCP_SHIELD_POST_STOP_HOOK:-/usr/local/bin/post-stop.sh}"

# ─────────────────────────────────────────────────────────────────────────────
# Logging Functions
# ─────────────────────────────────────────────────────────────────────────────
log_info() {
    echo "[${SCRIPT_NAME}] [INFO]  $(date -u '+%Y-%m-%dT%H:%M:%SZ') $*"
}

log_warn() {
    echo "[${SCRIPT_NAME}] [WARN]  $(date -u '+%Y-%m-%dT%H:%M:%SZ') $*" >&2
}

log_error() {
    echo "[${SCRIPT_NAME}] [ERROR] $(date -u '+%Y-%m-%dT%H:%M:%SZ') $*" >&2
}

log_debug() {
    if [[ "${MCP_SHIELD_DEBUG:-false}" == "true" ]]; then
        echo "[${SCRIPT_NAME}] [DEBUG] $(date -u '+%Y-%m-%dT%H:%M:%SZ') $*"
    fi
}

# ─────────────────────────────────────────────────────────────────────────────
# Signal Handling
# ─────────────────────────────────────────────────────────────────────────────
readonly MCP_SHIELD_PID_FILE="${MCP_SHIELD_TMP_DIR}/mcp-shield.pid"
readonly MCP_SHIELD_SHUTDOWN_TIMEOUT="${MCP_SHIELD_SHUTDOWN_TIMEOUT:-30}"
MCP_SHIELD_CHILD_PID=0

signal_handler() {
    local signal="$1"
    log_info "Received signal: ${signal}"
    
    if [[ ${MCP_SHIELD_CHILD_PID} -ne 0 ]]; then
        log_info "Forwarding ${signal} to child process (PID: ${MCP_SHIELD_CHILD_PID})"
        kill -"${signal}" "${MCP_SHIELD_CHILD_PID}" 2>/dev/null || true
        
        # Wait for graceful shutdown
        local timeout=${MCP_SHIELD_SHUTDOWN_TIMEOUT}
        local count=0
        while kill -0 "${MCP_SHIELD_CHILD_PID}" 2>/dev/null && [[ ${count} -lt ${timeout} ]]; do
            sleep 1
            ((count++))
        done
        
        if kill -0 "${MCP_SHIELD_CHILD_PID}" 2>/dev/null; then
            log_warn "Child process did not terminate gracefully, sending SIGKILL"
            kill -9 "${MCP_SHIELD_CHILD_PID}" 2>/dev/null || true
        fi
        
        wait "${MCP_SHIELD_CHILD_PID}" 2>/dev/null || true
    fi
    
    # Run post-stop hook if exists
    if [[ -x "${MCP_SHIELD_POST_STOP_HOOK}" ]]; then
        log_info "Running post-stop hook..."
        "${MCP_SHIELD_POST_STOP_HOOK}" || log_warn "Post-stop hook failed"
    fi
    
    log_info "Shutdown complete"
    exit 0
}

trap 'signal_handler SIGTERM' SIGTERM
trap 'signal_handler SIGINT' SIGINT
trap 'signal_handler SIGHUP' SIGHUP

# ─────────────────────────────────────────────────────────────────────────────
# Utility Functions
# ─────────────────────────────────────────────────────────────────────────────
ensure_directories() {
    log_info "Ensuring required directories exist..."
    
    local dirs=(
        "${MCP_SHIELD_DATA_DIR}"
        "${MCP_SHIELD_LOG_DIR}"
        "${MCP_SHIELD_TMP_DIR}"
        "${MCP_SHIELD_POLICIES_DIR}"
        "$(dirname "${MCP_SHIELD_CONFIG}")"
    )
    
    for dir in "${dirs[@]}"; do
        if [[ ! -d "${dir}" ]]; then
            mkdir -p "${dir}"
            log_debug "Created directory: ${dir}"
        fi
    done
    
    # Ensure proper ownership (if running as root initially)
    if [[ $(id -u) -eq 0 ]]; then
        chown -R "${MCP_SHIELD_UID}:${MCP_SHIELD_GID}" \
            "${MCP_SHIELD_DATA_DIR}" \
            "${MCP_SHIELD_LOG_DIR}" \
            "${MCP_SHIELD_TMP_DIR}" \
            "${MCP_SHIELD_POLICIES_DIR}" \
            "$(dirname "${MCP_SHIELD_CONFIG}")" 2>/dev/null || true
    fi
}

load_secrets() {
    log_info "Loading secrets from files..."
    
    # Load secrets from files (Docker secrets, Kubernetes secrets, etc.)
    local secret_files=(
        "/run/secrets/mcp_shield_jwt_secret:MCP_SHIELD_AUTH_JWT_SECRET"
        "/run/secrets/mcp_shield_jwks_url:MCP_SHIELD_AUTH_JWKS_URL"
        "/run/secrets/mcp_shield_db_password:MCP_SHIELD_DB_PASSWORD"
        "/run/secrets/mcp_shield_redis_password:MCP_SHIELD_REDIS_PASSWORD"
        "/run/secrets/mcp_shield_otlp_endpoint:OTEL_EXPORTER_OTLP_ENDPOINT"
        "/run/secrets/mcp_shield_otlp_headers:OTEL_EXPORTER_OTLP_HEADERS"
    )
    
    for secret_mapping in "${secret_files[@]}"; do
        local file="${secret_mapping%%:*}"
        local env_var="${secret_mapping##*:}"
        
        if [[ -f "${file}" ]] && [[ -z "${!env_var:-}" ]]; then
            export "${env_var}=$(cat "${file}" | tr -d '\n\r')"
            log_debug "Loaded secret from ${file} into ${env_var}"
        fi
    done
}

template_config() {
    log_info "Templating configuration from environment variables..."
    
    local config_template="${MCP_SHIELD_CONFIG}.template"
    local config_output="${MCP_SHIELD_CONFIG}"
    
    # If template exists, use it; otherwise create from default
    if [[ -f "${config_template}" ]]; then
        log_debug "Using template: ${config_template}"
    else
        # Create template from existing config
        cp "${config_output}" "${config_template}"
        log_debug "Created template from: ${config_output}"
    fi
    
    # Use envsubst to substitute environment variables
    if command -v envsubst >/dev/null 2>&1; then
        envsubst < "${config_template}" > "${config_output}"
        log_debug "Configuration templated to: ${config_output}"
    else
        log_warn "envsubst not available, using configuration as-is"
    fi
    
    # Validate configuration
    validate_config
}

validate_config() {
    log_info "Validating configuration..."
    
    if [[ ! -f "${MCP_SHIELD_CONFIG}" ]]; then
        log_error "Configuration file not found: ${MCP_SHIELD_CONFIG}"
        return 1
    fi
    
    # Basic TOML syntax check
    if command -v toml >/dev/null 2>&1; then
        if ! toml < "${MCP_SHIELD_CONFIG}" >/dev/null 2>&1; then
            log_error "Configuration file has invalid TOML syntax"
            return 1
        fi
    fi
    
    # Check required fields
    local required_fields=(
        "server.bind_addr"
        "server.log_level"
    )
    
    for field in "${required_fields[@]}"; do
        if ! grep -q "^\s*${field}\s*=" "${MCP_SHIELD_CONFIG}"; then
            log_warn "Recommended configuration field not found: ${field}"
        fi
    done
    
    # Validate auth configuration if enabled
    if grep -q "^\s*enabled\s*=\s*true" "${MCP_SHIELD_CONFIG}" 2>/dev/null; then
        if ! grep -q "^\s*jwt_secret\s*=" "${MCP_SHIELD_CONFIG}" && [[ -z "${MCP_SHIELD_AUTH_JWT_SECRET:-}" ]]; then
            if ! grep -q "^\s*jwks_url\s*=" "${MCP_SHIELD_CONFIG}" && [[ -z "${MCP_SHIELD_AUTH_JWKS_URL:-}" ]]; then
                log_error "Authentication enabled but no JWT secret or JWKS URL configured"
                return 1
            fi
        fi
    fi
    
    log_debug "Configuration validation passed"
    return 0
}

run_migrations() {
    if [[ "${MCP_SHIELD_RUN_MIGRATIONS:-false}" != "true" ]]; then
        log_debug "Database migrations disabled"
        return 0
    fi
    
    log_info "Running database migrations..."
    
    # Check if migration binary exists
    if [[ -x "/usr/local/bin/mcp-shield-migrate" ]]; then
        /usr/local/bin/mcp-shield-migrate up || {
            log_error "Database migrations failed"
            return 1
        }
        log_info "Database migrations completed successfully"
    else
        log_warn "Migration binary not found, skipping migrations"
    fi
}

run_pre_start_hook() {
    if [[ -x "${MCP_SHIELD_PRE_START_HOOK}" ]]; then
        log_info "Running pre-start hook..."
        "${MCP_SHIELD_PRE_START_HOOK}" || {
            log_error "Pre-start hook failed"
            return 1
        }
        log_info "Pre-start hook completed"
    else
        log_debug "No pre-start hook found at ${MCP_SHIELD_PRE_START_HOOK}"
    fi
}

drop_privileges() {
    # If running as root, drop to non-root user
    if [[ $(id -u) -eq 0 ]]; then
        log_info "Dropping privileges to user ${MCP_SHIELD_USER} (UID: ${MCP_SHIELD_UID})"
        
        # Ensure user exists
        if ! id "${MCP_SHIELD_USER}" >/dev/null 2>&1; then
            groupadd --gid "${MCP_SHIELD_GID}" "${MCP_SHIELD_USER}" 2>/dev/null || true
            useradd --uid "${MCP_SHIELD_UID}" --gid "${MCP_SHIELD_GID}" \
                --create-home --home-dir "/home/${MCP_SHIELD_USER}" \
                --shell /sbin/nologin "${MCP_SHIELD_USER}" 2>/dev/null || true
        fi
        
        # Re-execute as non-root user
        exec gosu "${MCP_SHIELD_USER}:${MCP_SHIELD_GROUP}" "$@"
    fi
}

# ─────────────────────────────────────────────────────────────────────────────
# Main Entry Point
# ─────────────────────────────────────────────────────────────────────────────
main() {
    log_info "Starting MCP-Shield entrypoint..."
    log_info "Version: ${MCP_SHIELD_VERSION:-unknown}"
    log_info "Build: ${MCP_SHIELD_BUILD_DATE:-unknown}"
    log_info "Revision: ${MCP_SHIELD_VCS_REF:-unknown}"
    
    # Print environment (filtered)
    log_debug "Environment variables (filtered):"
    env | grep -E '^(MCP_SHIELD_|OTEL_|RUST_)' | sort | while IFS= read -r line; do
        # Mask secrets
        if [[ "${line}" =~ (SECRET|PASSWORD|TOKEN|KEY)= ]]; then
            log_debug "  ${line%%=*}=***REDACTED***"
        else
            log_debug "  ${line}"
        fi
    done
    
    # Ensure directories exist
    ensure_directories
    
    # Load secrets from files
    load_secrets
    
    # Template configuration
    template_config
    
    # Run database migrations (if enabled)
    run_migrations
    
    # Run pre-start hook
    run_pre_start_hook
    
    # Prepare command
    local cmd=("$@")
    log_info "Executing: ${cmd[*]}"
    
    # Drop privileges and execute
    if [[ $(id -u) -eq 0 ]]; then
        drop_privileges "${cmd[@]}"
    else
        # Already non-root, execute directly
        exec "${cmd[@]}" &
        MCP_SHIELD_CHILD_PID=$!
        wait ${MCP_SHIELD_CHILD_PID}
    fi
}

# ─────────────────────────────────────────────────────────────────────────────
# Execute Main
# ─────────────────────────────────────────────────────────────────────────────
main "$@"
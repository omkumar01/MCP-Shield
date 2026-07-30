#!/bin/bash
# ┌──────────────────────────────────────────────────────────────────────────────┐
# │ MCP-Shield Health Check Script                                              │
# │                                                                              │
# │ Performs comprehensive health checks for Docker/Kubernetes liveness         │
# │ and readiness probes.                                                        │
# └──────────────────────────────────────────────────────────────────────────────┘

set -euo pipefail

# ─────────────────────────────────────────────────────────────────────────────
# Configuration
# ─────────────────────────────────────────────────────────────────────────────
readonly SCRIPT_NAME="$(basename "$0")"
readonly HEALTH_PORT="${MCP_SHIELD_HEALTH_PORT:-9091}"
readonly METRICS_PORT="${MCP_SHIELD_METRICS_PORT:-9090}"
readonly HTTP_PORT="${MCP_SHIELD_HTTP_PORT:-8080}"
readonly TIMEOUT="${MCP_SHIELD_HEALTH_TIMEOUT:-5}"
readonly CHECK_TYPE="${1:-health}"  # health, ready, live

# Endpoints
readonly HEALTH_ENDPOINT="http://localhost:${HEALTH_PORT}/health"
readonly READY_ENDPOINT="http://localhost:${HEALTH_PORT}/ready"
readonly LIVE_ENDPOINT="http://localhost:${HEALTH_PORT}/live"
readonly METRICS_ENDPOINT="http://localhost:${METRICS_PORT}/metrics"
readonly MCP_ENDPOINT="http://localhost:${HTTP_PORT}/mcp"

# ─────────────────────────────────────────────────────────────────────────────
# Logging
# ─────────────────────────────────────────────────────────────────────────────
log_info() {
    echo "[${SCRIPT_NAME}] [INFO] $(date -u +"%Y-%m-%dT%H:%M:%SZ") $*" >&2
}

log_warn() {
    echo "[${SCRIPT_NAME}] [WARN] $(date -u +"%Y-%m-%dT%H:%M:%SZ") $*" >&2
}

log_error() {
    echo "[${SCRIPT_NAME}] [ERROR] $(date -u +"%Y-%m-%dT%H:%M:%SZ") $*" >&2
}

log_debug() {
    if [[ "${MCP_SHIELD_DEBUG:-false}" == "true" ]]; then
        echo "[${SCRIPT_NAME}] [DEBUG] $(date -u +"%Y-%m-%dT%H:%M:%SZ") $*" >&2
    fi
}

# ─────────────────────────────────────────────────────────────────────────────
# HTTP Check Function
# ─────────────────────────────────────────────────────────────────────────────
http_check() {
    local url="$1"
    local expected_status="${2:-200}"
    local description="${3:-$url}"
    
    log_debug "Checking ${description} at ${url}"
    
    local response
    local http_code
    
    response=$(curl -sf --max-time "${TIMEOUT}" \
        -H "Accept: application/json" \
        -w "\n%{http_code}" \
        "${url}" 2>/dev/null) || {
        log_error "Failed to connect to ${description}"
        return 1
    }
    
    http_code=$(echo "${response}" | tail -n1)
    local body=$(echo "${response}" | head -n -1)
    
    if [[ "${http_code}" -eq "${expected_status}" ]]; then
        log_debug "${description} OK (HTTP ${http_code})"
        return 0
    else
        log_error "${description} returned HTTP ${http_code} (expected ${expected_status})"
        log_debug "Response: ${body}"
        return 1
    fi
}

# ─────────────────────────────────────────────────────────────────────────────
# Health Check Types
# ─────────────────────────────────────────────────────────────────────────────
check_live() {
    log_info "Liveness check..."
    http_check "${LIVE_ENDPOINT}" 200 "Liveness endpoint"
}

check_ready() {
    log_info "Readiness check..."
    http_check "${READY_ENDPOINT}" 200 "Readiness endpoint"
}

check_health() {
    log_info "Health check..."
    http_check "${HEALTH_ENDPOINT}" 200 "Health endpoint"
}

check_metrics() {
    log_info "Metrics endpoint check..."
    http_check "${METRICS_ENDPOINT}" 200 "Metrics endpoint"
}

check_mcp() {
    log_info "MCP endpoint check..."
    
    local response
    response=$(curl -sf --max-time "${TIMEOUT}" \
        -H "Content-Type: application/json" \
        -H "Accept: application/json" \
        -d '{"jsonrpc":"2.0","id":1,"method":"ping"}' \
        -w "\n%{http_code}" \
        "${MCP_ENDPOINT}" 2>/dev/null) || {
        log_warn "MCP endpoint not responding (may require auth)"
        return 0  # Don't fail health check for auth-required endpoints
    }
    
    local http_code=$(echo "${response}" | tail -n1)
    if [[ "${http_code}" -eq 200 ]] || [[ "${http_code}" -eq 401 ]] || [[ "${http_code}" -eq 403 ]]; then
        log_debug "MCP endpoint responsive (HTTP ${http_code})"
        return 0
    else
        log_warn "MCP endpoint returned unexpected status: ${http_code}"
        return 0  # Don't fail health check
    fi
}

# ─────────────────────────────────────────────────────────────────────────────
# Main
# ─────────────────────────────────────────────────────────────────────────────
main() {
    log_info "Starting health check (type: ${CHECK_TYPE})"
    
    case "${CHECK_TYPE}" in
        live|liveness)
            check_live
            ;;
        ready|readiness)
            check_ready
            ;;
        health|healthy)
            check_health
            ;;
        metrics)
            check_metrics
            ;;
        mcp)
            check_mcp
            ;;
        all)
            local failed=0
            check_live || ((failed++))
            check_ready || ((failed++))
            check_health || ((failed++))
            check_metrics || ((failed++))
            check_mcp || ((failed++))
            
            if [[ ${failed} -gt 0 ]]; then
                log_error "${failed} health check(s) failed"
                exit 1
            fi
            log_info "All health checks passed"
            ;;
        *)
            log_error "Unknown check type: ${CHECK_TYPE}"
            log_error "Usage: $0 {live|ready|health|metrics|mcp|all}"
            exit 1
            ;;
    esac
}

main "$@"
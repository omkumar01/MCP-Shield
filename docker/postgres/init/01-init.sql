# ┌──────────────────────────────────────────────────────────────────────────────┐
# │ MCP-Shield PostgreSQL Initialization Script                                  │
# │                                                                              │
# │ Creates schema, users, and initial data for the control plane (Phase 4)     │
# └──────────────────────────────────────────────────────────────────────────────┘

-- Enable required extensions
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS "pgcrypto";
CREATE EXTENSION IF NOT EXISTS "citext";

-- Create application user (if not exists)
DO $$
BEGIN
    IF NOT EXISTS (SELECT FROM pg_catalog.pg_roles WHERE rolname = 'mcp_shield_app') THEN
        CREATE ROLE mcp_shield_app WITH LOGIN PASSWORD 'changeme';
    END IF;
END
$$;

-- Create schemas
CREATE SCHEMA IF NOT EXISTS mcp_shield AUTHORIZATION mcp_shield_app;
CREATE SCHEMA IF NOT EXISTS mcp_shield_audit AUTHORIZATION mcp_shield_app;

-- Set search path
SET search_path TO mcp_shield, mcp_shield_audit, public;

-- ═══════════════════════════════════════════════════════════════════════════════
-- Tenants Table
-- ═══════════════════════════════════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS tenants (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name CITEXT NOT NULL UNIQUE,
    display_name VARCHAR(255) NOT NULL,
    description TEXT,
    status VARCHAR(50) NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'suspended', 'deleted')),
    settings JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_tenants_status ON tenants(status);
CREATE INDEX IF NOT EXISTS idx_tenants_created_at ON tenants(created_at);

-- ═══════════════════════════════════════════════════════════════════════════════
-- Users Table
-- ═══════════════════════════════════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS users (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    email CITEXT NOT NULL,
    username CITEXT NOT NULL,
    display_name VARCHAR(255),
    password_hash VARCHAR(255) NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'inactive', 'locked', 'deleted')),
    roles JSONB NOT NULL DEFAULT '[]',
    mfa_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    mfa_secret VARCHAR(255),
    last_login_at TIMESTAMPTZ,
    failed_login_attempts INT NOT NULL DEFAULT 0,
    locked_until TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at TIMESTAMPTZ,
    UNIQUE (tenant_id, email),
    UNIQUE (tenant_id, username)
);

CREATE INDEX IF NOT EXISTS idx_users_tenant_id ON users(tenant_id);
CREATE INDEX IF NOT EXISTS idx_users_email ON users(email);
CREATE INDEX IF NOT EXISTS idx_users_status ON users(status);

-- ═══════════════════════════════════════════════════════════════════════════════
-- API Keys Table
-- ═══════════════════════════════════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS api_keys (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    name VARCHAR(255) NOT NULL,
    key_hash VARCHAR(255) NOT NULL,
    key_prefix VARCHAR(8) NOT NULL,
    scopes JSONB NOT NULL DEFAULT '[]',
    expires_at TIMESTAMPTZ,
    last_used_at TIMESTAMPTZ,
    usage_count BIGINT NOT NULL DEFAULT 0,
    status VARCHAR(50) NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'revoked', 'expired')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    revoked_at TIMESTAMPTZ,
    revoked_by UUID REFERENCES users(id)
);

CREATE INDEX IF NOT EXISTS idx_api_keys_tenant_id ON api_keys(tenant_id);
CREATE INDEX IF NOT EXISTS idx_api_keys_key_prefix ON api_keys(key_prefix);
CREATE INDEX IF NOT EXISTS idx_api_keys_status ON api_keys(status);

-- ═══════════════════════════════════════════════════════════════════════════════
-- Upstream Servers Table
-- ═══════════════════════════════════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS upstream_servers (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    url VARCHAR(500) NOT NULL,
    transport VARCHAR(50) NOT NULL DEFAULT 'streamable_http' CHECK (transport IN ('stdio', 'streamable_http', 'sse')),
    auth_config JSONB,
    headers JSONB NOT NULL DEFAULT '{}',
    timeout_seconds INT NOT NULL DEFAULT 30,
    max_concurrent_requests INT NOT NULL DEFAULT 100,
    health_check_path VARCHAR(255) DEFAULT '/health',
    health_check_interval_seconds INT NOT NULL DEFAULT 30,
    status VARCHAR(50) NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'inactive', 'unhealthy', 'draining')),
    circuit_breaker_config JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at TIMESTAMPTZ,
    UNIQUE (tenant_id, name)
);

CREATE INDEX IF NOT EXISTS idx_upstream_servers_tenant_id ON upstream_servers(tenant_id);
CREATE INDEX IF NOT EXISTS idx_upstream_servers_status ON upstream_servers(status);

-- ═══════════════════════════════════════════════════════════════════════════════
-- Tools Registry Table
-- ═══════════════════════════════════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS tools (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    upstream_server_id UUID NOT NULL REFERENCES upstream_servers(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    qualified_name VARCHAR(512) NOT NULL, -- prefix:name format
    description TEXT,
    input_schema JSONB NOT NULL,
    output_schema JSONB,
    annotations JSONB NOT NULL DEFAULT '{}',
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    version VARCHAR(50),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (tenant_id, qualified_name)
);

CREATE INDEX IF NOT EXISTS idx_tools_tenant_id ON tools(tenant_id);
CREATE INDEX IF NOT EXISTS idx_tools_upstream_server_id ON tools(upstream_server_id);
CREATE INDEX IF NOT EXISTS idx_tools_qualified_name ON tools(qualified_name);

-- ═══════════════════════════════════════════════════════════════════════════════
-- Policies Table (Cedar Policies)
-- ═══════════════════════════════════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS policies (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    policy_text TEXT NOT NULL, -- Cedar policy source
    policy_hash VARCHAR(64) NOT NULL, -- SHA256 of policy_text
    version INT NOT NULL DEFAULT 1,
    status VARCHAR(50) NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'active', 'deprecated', 'archived')),
    effect VARCHAR(50) NOT NULL CHECK (effect IN ('permit', 'forbid')),
    priority INT NOT NULL DEFAULT 0,
    conditions JSONB NOT NULL DEFAULT '{}',
    created_by UUID REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    activated_at TIMESTAMPTZ,
    deprecated_at TIMESTAMPTZ,
    UNIQUE (tenant_id, name, version)
);

CREATE INDEX IF NOT EXISTS idx_policies_tenant_id ON policies(tenant_id);
CREATE INDEX IF NOT EXISTS idx_policies_status ON policies(status);
CREATE INDEX IF NOT EXISTS idx_policies_policy_hash ON policies(policy_hash);

-- ═══════════════════════════════════════════════════════════════════════════════
-- Sessions Table
-- ═══════════════════════════════════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS sessions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    api_key_id UUID REFERENCES api_keys(id) ON DELETE SET NULL,
    session_id VARCHAR(255) NOT NULL UNIQUE,
    context_lock JSONB, -- Session context locking (Phase 2)
    metadata JSONB NOT NULL DEFAULT '{}',
    status VARCHAR(50) NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'expired', 'revoked', 'locked')),
    expires_at TIMESTAMPTZ NOT NULL,
    last_activity_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    revoked_at TIMESTAMPTZ,
    revoked_by UUID REFERENCES users(id)
);

CREATE INDEX IF NOT EXISTS idx_sessions_tenant_id ON sessions(tenant_id);
CREATE INDEX IF NOT EXISTS idx_sessions_session_id ON sessions(session_id);
CREATE INDEX IF NOT EXISTS idx_sessions_status ON sessions(status);
CREATE INDEX IF NOT EXISTS idx_sessions_expires_at ON sessions(expires_at);

-- ═══════════════════════════════════════════════════════════════════════════════
-- Audit Logs Table (Partitioned by Month)
-- ═══════════════════════════════════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS audit_logs (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    session_id UUID REFERENCES sessions(id) ON DELETE SET NULL,
    timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    event_type VARCHAR(100) NOT NULL,
    action VARCHAR(100) NOT NULL,
    resource_type VARCHAR(100),
    resource_id UUID,
    principal_type VARCHAR(50), -- user, api_key, system
    principal_id UUID,
    decision VARCHAR(50) CHECK (decision IN ('allow', 'deny', 'error')),
    policy_ids UUID[],
    request JSONB,
    response JSONB,
    metadata JSONB NOT NULL DEFAULT '{}',
    error_message TEXT
) PARTITION BY RANGE (timestamp);

CREATE INDEX IF NOT EXISTS idx_audit_logs_tenant_id ON audit_logs(tenant_id);
CREATE INDEX IF NOT EXISTS idx_audit_logs_timestamp ON audit_logs(timestamp);
CREATE INDEX IF NOT EXISTS idx_audit_logs_event_type ON audit_logs(event_type);
CREATE INDEX IF NOT EXISTS idx_audit_logs_decision ON audit_logs(decision);
CREATE INDEX IF NOT EXISTS idx_audit_logs_principal ON audit_logs(principal_type, principal_id);

-- Create monthly partitions for current and next year
DO $$
DECLARE
    start_date DATE := date_trunc('month', CURRENT_DATE);
    end_date DATE := start_date + INTERVAL '14 months';
    partition_start DATE;
    partition_end DATE;
    partition_name TEXT;
BEGIN
    WHILE start_date < end_date LOOP
        partition_start := start_date;
        partition_end := start_date + INTERVAL '1 month';
        partition_name := 'audit_logs_' || to_char(partition_start, 'YYYY_MM');
        
        EXECUTE format(
            'CREATE TABLE IF NOT EXISTS %I PARTITION OF audit_logs FOR VALUES FROM (%L) TO (%L)',
            partition_name, partition_start, partition_end
        );
        
        start_date := partition_end;
    END LOOP;
END
$$;

-- ═══════════════════════════════════════════════════════════════════════════════
-- Rate Limit Rules Table
-- ═══════════════════════════════════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS rate_limit_rules (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    scope VARCHAR(50) NOT NULL CHECK (scope IN ('global', 'tenant', 'user', 'api_key', 'ip', 'tool')),
    scope_value VARCHAR(255),
    algorithm VARCHAR(50) NOT NULL DEFAULT 'token_bucket' CHECK (algorithm IN ('token_bucket', 'sliding_window', 'fixed_window')),
    requests_per_window BIGINT NOT NULL,
    window_seconds INT NOT NULL,
    burst_allowance BIGINT,
    action VARCHAR(50) NOT NULL DEFAULT 'reject' CHECK (action IN ('reject', 'throttle', 'queue')),
    priority INT NOT NULL DEFAULT 0,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_rate_limit_rules_tenant_id ON rate_limit_rules(tenant_id);
CREATE INDEX IF NOT EXISTS idx_rate_limit_rules_scope ON rate_limit_rules(scope, scope_value);

-- ═══════════════════════════════════════════════════════════════════════════════
-- Triggers for Updated At
-- ═══════════════════════════════════════════════════════════════════════════════
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER update_tenants_updated_at BEFORE UPDATE ON tenants FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_users_updated_at BEFORE UPDATE ON users FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_api_keys_updated_at BEFORE UPDATE ON api_keys FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_upstream_servers_updated_at BEFORE UPDATE ON upstream_servers FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_tools_updated_at BEFORE UPDATE ON tools FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_policies_updated_at BEFORE UPDATE ON policies FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE TRIGGER update_rate_limit_rules_updated_at BEFORE UPDATE ON rate_limit_rules FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- ═══════════════════════════════════════════════════════════════════════════════
-- Default Data
-- ═══════════════════════════════════════════════════════════════════════════════
-- Default tenant
INSERT INTO tenants (name, display_name, description, settings)
VALUES ('default', 'Default Tenant', 'Default tenant for local development', '{}')
ON CONFLICT (name) DO NOTHING;

-- Grant permissions
GRANT USAGE ON SCHEMA mcp_shield TO mcp_shield_app;
GRANT USAGE ON SCHEMA mcp_shield_audit TO mcp_shield_app;
GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA mcp_shield TO mcp_shield_app;
GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA mcp_shield_audit TO mcp_shield_app;
GRANT ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA mcp_shield TO mcp_shield_app;
GRANT ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA mcp_shield_audit TO mcp_shield_app;

-- Default privileges for future objects
ALTER DEFAULT PRIVILEGES IN SCHEMA mcp_shield GRANT ALL ON TABLES TO mcp_shield_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA mcp_shield GRANT ALL ON SEQUENCES TO mcp_shield_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA mcp_shield_audit GRANT ALL ON TABLES TO mcp_shield_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA mcp_shield_audit GRANT ALL ON SEQUENCES TO mcp_shield_app;
# ┌──────────────────────────────────────────────────────────────────────────────┐
# │ MCP-Shield Docker Configuration                                              │
# │                                                                              │
# │ Production-ready Docker support for MCP-Shield.                             │
# └──────────────────────────────────────────────────────────────────────────────┘

---

## Files

| File | Description |
|------|-------------|
| `Dockerfile` | Multi-stage production Dockerfile |
| `Dockerfile.echo` | Echo test server Dockerfile |
| `entrypoint/entrypoint.sh` | Container entrypoint script |
| `healthcheck/healthcheck.sh` | Health check script |

---

## Building

### Local Build (Single Arch)
```bash
docker build -t mcp-shield:latest -f docker/Dockerfile .
```

### Multi-Arch Build (Production)
```bash
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  -t ghcr.io/your-org/mcp-shield:v0.1.0 \
  -t ghcr.io/your-org/mcp-shield:latest \
  --push \
  -f docker/Dockerfile \
  --build-arg BUILD_DATE=$(date -u +"%Y-%m-%dT%H:%M:%SZ") \
  --build-arg BUILD_VERSION=v0.1.0 \
  --build-arg VCS_REF=$(git rev-parse HEAD) \
  --build-arg VCS_URL=https://github.com/your-org/mcp-shield \
  .
```

### Build with BuildKit (Recommended)
```bash
DOCKER_BUILDKIT=1 docker build \
  --tag mcp-shield:latest \
  --file docker/Dockerfile \
  .
```

---

## Running

### Development
```bash
docker run --rm -it \
  -p 8080:8080 \
  -p 9090:9090 \
  -p 9091:9091 \
  -v $(pwd)/config:/etc/mcp-shield/config:ro \
  -v $(pwd)/policies:/etc/mcp-shield/policies:ro \
  mcp-shield:latest
```

### Production
```bash
docker run -d \
  --name mcp-shield \
  --restart unless-stopped \
  -p 8080:8080 \
  -p 9090:9090 \
  --user 65532:65532 \
  --read-only \
  --tmpfs /tmp --tmpfs /var/run --tmpfs /var/lib/mcp-shield/data \
  --cap-drop ALL \
  --security-opt no-new-privileges:true \
  -e MCP_SHIELD_AUTH_ENABLED=true \
  -e MCP_SHIELD_AUTH_JWKS_URL=https://auth.example.com/.well-known/jwks.json \
  -e MCP_SHIELD_AUTH_ISSUER=https://auth.example.com \
  -v /etc/mcp-shield/config:/etc/mcp-shield/config:ro \
  -v /etc/mcp-shield/policies:/etc/mcp-shield/policies:ro \
  ghcr.io/your-org/mcp-shield:v0.1.0
```

### With Docker Secrets
```bash
# Create secrets
echo "your-jwt-secret" | docker secret create mcp_shield_jwt_secret -
echo "your-db-password" | docker secret create mcp_shield_db_password -

# Run with secrets
docker service create \
  --name mcp-shield \
  --secret mcp_shield_jwt_secret \
  --secret mcp_shield_db_password \
  -p 8080:8080 \
  ghcr.io/your-org/mcp-shield:v0.1.0
```

---

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `MCP_SHIELD_CONFIG` | Config file path | `/etc/mcp-shield/config/default.toml` |
| `MCP_SHIELD_SERVER_BIND_ADDR` | Bind address | `0.0.0.0:8080` |
| `MCP_SHIELD_SERVER_LOG_LEVEL` | Log level | `info` |
| `MCP_SHIELD_AUTH_ENABLED` | Enable auth | `false` |
| `MCP_SHIELD_AUTH_JWT_SECRET` | JWT HMAC secret | - |
| `MCP_SHIELD_AUTH_JWKS_URL` | JWKS endpoint | - |
| `MCP_SHIELD_AUTH_ISSUER` | Token issuer | - |
| `MCP_SHIELD_TELEMETRY_OTLP_ENDPOINT` | OTLP collector | - |
| `OTEL_SERVICE_NAME` | Service name | `mcp-shield` |
| `RUST_LOG` | Rust log level | `info` |

### Secret Files (from /run/secrets/)
| File | Environment Variable |
|------|---------------------|
| `mcp_shield_jwt_secret` | `MCP_SHIELD_AUTH_JWT_SECRET` |
| `mcp_shield_jwks_url` | `MCP_SHIELD_AUTH_JWKS_URL` |
| `mcp_shield_db_password` | `MCP_SHIELD_DB_PASSWORD` |
| `mcp_shield_redis_password` | `MCP_SHIELD_REDIS_PASSWORD` |
| `mcp_shield_tls_cert` | `MCP_SHIELD_TLS_CERT` |
| `mcp_shield_tls_key` | `MCP_SHIELD_TLS_KEY` |
| `mcp_shield_tls_ca` | `MCP_SHIELD_TLS_CA` |

---

## Health Checks

```bash
# Liveness
curl http://localhost:9091/live

# Readiness
curl http://localhost:9091/ready

# Full health
curl http://localhost:9091/health

# Metrics
curl http://localhost:9090/metrics
```

### Docker Health Check
```bash
# View health status
docker ps --filter "name=mcp-shield" --format "table {{.Names}}\t{{.Status}}"

# Manual health check
docker exec mcp-shield /usr/local/bin/healthcheck.sh health
```

---

## Security Features

### Non-Root User
- UID/GID: 65532 (nonroot)
- Home: `/home/nonroot`
- No login shell

### Read-Only Root Filesystem
```dockerfile
# In Dockerfile
USER nonroot
# At runtime
docker run --read-only ...
```

### Dropped Capabilities
```dockerfile
# All capabilities dropped
cap_drop:
  - ALL
```

### No New Privileges
```dockerfile
security_opt:
  - no-new-privileges:true
```

### Seccomp Profile
```dockerfile
# Default: RuntimeDefault
# Custom profile supported
seccompProfile:
  type: RuntimeDefault
```

---

## Image Layers

The multi-stage build creates minimal layers:

1. **deps** - Cached dependencies (changes only when Cargo.toml changes)
2. **builder** - Compiles the application
3. **sbom** - Generates Software Bill of Materials
4. **runtime** - Minimal runtime (Debian slim ~80MB)

### Layer Optimization
- Dependencies cached separately from source
- BuildKit cache mounts for cargo registry/target
- Binary stripped in builder stage
- Only runtime dependencies in final image

---

## SBOM Generation

The build generates SBOMs in multiple formats:

```bash
# From the sbom stage
docker build --target sbom -t mcp-shield:sbom -f docker/Dockerfile .
docker run --rm mcp-shield:sbom cat /sbom.cyclonedx.json > sbom.cyclonedx.json
docker run --rm mcp-shield:sbom cat /sbom.spdx.json > sbom.spdx.json
docker run --rm mcp-shield:sbom cat /sbom.table.txt > sbom.table.txt
```

### Verify SBOM
```bash
# Using syft
syft packages dir:. -o table

# Using trivy
trivy image --format json --output trivy-report.json mcp-shield:latest
```

---

## Image Signing (Cosign)

```bash
# Sign image
cosign sign --yes \
  --annotations "version=v0.1.0" \
  --annotations "git-sha=$(git rev-parse HEAD)" \
  ghcr.io/your-org/mcp-shield:v0.1.0

# Verify signature
cosign verify \
  --certificate-identity-regexp=".*" \
  --certificate-oidc-issuer-regexp=".*" \
  ghcr.io/your-org/mcp-shield:v0.1.0

# Attach SBOM
cosign attach sbom --sbom sbom.spdx.json \
  ghcr.io/your-org/mcp-shield:v0.1.0
```

---

## Vulnerability Scanning

### Trivy
```bash
# Scan image
trivy image --severity HIGH,CRITICAL ghcr.io/your-org/mcp-shield:v0.1.0

# Scan with SARIF output for GitHub
trivy image --format sarif --output trivy.sarif \
  --severity HIGH,CRITICAL \
  ghcr.io/your-org/mcp-shield:v0.1.0
```

### Grype
```bash
grype ghcr.io/your-org/mcp-shield:v0.1.0 --fail-on high
```

---

## Troubleshooting

### Container Won't Start
```bash
# Check logs
docker logs mcp-shield

# Run interactively
docker run --rm -it --entrypoint /bin/bash mcp-shield:latest
```

### Permission Issues
```bash
# Ensure volumes have correct ownership
docker run --rm -v $(pwd)/config:/etc/mcp-shield/config \
  alpine chown -R 65532:65532 /etc/mcp-shield/config
```

### Health Check Failing
```bash
# Debug health check
docker exec mcp-shield /usr/local/bin/healthcheck.sh all

# Check endpoints manually
docker exec mcp-shield curl -v http://localhost:9091/health
```

---

## Best Practices

1. **Use specific tags** - Never use `latest` in production
2. **Pin digests** - Use `image@sha256:...` for immutable deployments
3. **Scan regularly** - Automate vulnerability scanning in CI/CD
4. **Sign images** - Use Cosign/Sigstore for supply chain security
5. **Generate SBOMs** - Attach to images for compliance
6. **Use secrets** - Never put secrets in environment variables
7. **Limit resources** - Set CPU/memory limits
8. **Enable health checks** - Configure liveness/readiness probes
9. **Run as non-root** - Always use non-root user
10. **Drop capabilities** - Drop all unnecessary capabilities
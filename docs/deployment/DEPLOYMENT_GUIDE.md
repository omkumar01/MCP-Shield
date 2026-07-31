# ┌──────────────────────────────────────────────────────────────────────────────┐
# │ MCP-Shield Deployment Guide                                                  │
# │                                                                              │
# │ Complete deployment documentation for MCP-Shield.                            │
# └──────────────────────────────────────────────────────────────────────────────┘

# MCP-Shield Deployment Guide

## Table of Contents

1. [Overview](#overview)
2. [Prerequisites](#prerequisites)
3. [Local Development](#local-development)
4. [Docker Deployment](#docker-deployment)
5. [Docker Compose Deployment](#docker-compose-deployment)
6. [Kubernetes Deployment (Helm)](#kubernetes-deployment-helm)
7. [Configuration](#configuration)
8. [Secrets Management](#secrets-management)
9. [TLS/SSL Configuration](#tlsssl-configuration)
10. [Upgrades & Rollbacks](#upgrades--rollbacks)
11. [Backup & Disaster Recovery](#backup--disaster-recovery)
12. [Scaling](#scaling)
13. [Monitoring & Observability](#monitoring--observability)
14. [Troubleshooting](#troubleshooting)
15. [Production Hardening](#production-hardening)

---

## Overview

MCP-Shield is a Layer 7 protocol-aware security gateway for the Model Context Protocol (MCP). It sits between MCP clients (AI applications) and MCP servers, providing:

- **Protocol-aware inspection** - JSON-RPC 2.0 parsing and validation
- **Authentication & Authorization** - OAuth 2.1, JWT validation, fine-grained scopes
- **Policy Enforcement** - Amazon Cedar ABAC policies (Phase 2)
- **Audit Logging** - Comprehensive request/response logging
- **Runtime Security** - Prompt injection prevention, egress sanitization (Phase 3)

### Deployment Options

| Environment | Method | Use Case |
|-------------|--------|----------|
| Local Development | Docker Compose + Cargo | Development, testing |
| Staging | Docker Compose / Kubernetes | Integration testing |
| Production | Kubernetes (Helm) | Enterprise production |
| Self-hosted | Docker / Kubernetes | On-premises, air-gapped |

---

## Prerequisites

### System Requirements

| Component | Minimum | Recommended |
|-----------|---------|-------------|
| CPU | 2 cores | 4+ cores |
| Memory | 2 GB | 8+ GB |
| Disk | 10 GB | 50+ GB |
| Network | 1 Gbps | 10 Gbps |

### Software Dependencies

#### For Local Development
- **Rust** 1.97+ (via `rustup`)
- **Docker** 24+ and **Docker Compose** 2+
- **Git** 2.40+
- **Make** 4.3+

#### For Kubernetes Deployment
- **Kubernetes** 1.28+
- **Helm** 3.14+
- **kubectl** configured for target cluster
- **Container Runtime** with CRI support (containerd, CRI-O)

#### For Production
- **Ingress Controller** (NGINX, Traefik, Envoy Gateway)
- **Cert-Manager** for TLS certificates
- **Prometheus Operator** for monitoring
- **External Secrets Operator** / **HashiCorp Vault** for secrets
- **Network Policy CNI** (Calico, Cilium, Weave)

---

## Local Development

### Quick Start

```bash
# Clone repository
git clone https://github.com/mcp-shield/mcp-shield
cd mcp-shield

# Install Rust toolchain
rustup toolchain install 1.97

# Build and run
cargo build --release
cargo run -- --config config/default.toml
```

### Using Docker Compose (Recommended)

```bash
# Start full development stack
docker-compose -f docker-compose.yml -f docker-compose.dev.yml up -d

# View logs
docker-compose -f docker-compose.yml -f docker-compose.dev.yml logs -f mcp-shield

# Stop stack
docker-compose -f docker-compose.yml -f docker-compose.dev.yml down -v
```

This starts:
- **MCP-Shield Gateway** (port 8080)
- **PostgreSQL** (port 5432)
- **Redis** (port 6379)
- **OpenTelemetry Collector** (ports 4317, 4318)
- **Prometheus** (port 9090)
- **Grafana** (port 3000)
- **Loki** (port 3100)
- **Jaeger** (port 16686)
- **MCP Echo Server** (port 8081)

### Development Tools

```bash
# Using Makefile
make dev              # Hot-reload development server
make dev-docker       # Full Docker Compose stack
make test             # Run all tests
make lint             # Format + clippy
make docker-build     # Build Docker image

# Access services
# Gateway:      http://localhost:8080
# Metrics:      http://localhost:9090/metrics
# Health:       http://localhost:9091/health
# Grafana:      http://localhost:3000 (admin/admin)
# Jaeger:       http://localhost:16686
# Prometheus:   http://localhost:9090
```

---

## Docker Deployment

### Building the Image

```bash
# Build locally
docker build -t mcp-shield:latest -f docker/Dockerfile .

# Multi-arch build (requires buildx)
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  -t ghcr.io/your-org/mcp-shield:v0.1.0 \
  -t ghcr.io/your-org/mcp-shield:latest \
  --push \
  -f docker/Dockerfile .
```

### Running the Container

```bash
# Basic run
docker run -d \
  --name mcp-shield \
  -p 8080:8080 \
  -p 9090:9090 \
  -p 9091:9091 \
  -v $(pwd)/config:/etc/mcp-shield/config:ro \
  -v $(pwd)/policies:/etc/mcp-shield/policies:ro \
  mcp-shield:latest

# With environment variables
docker run -d \
  --name mcp-shield \
  -p 8080:8080 \
  -p 9090:9090 \
  -e MCP_SHIELD_AUTH_ENABLED=true \
  -e MCP_SHIELD_AUTH_JWT_SECRET=your-secret \
  -e MCP_SHIELD_AUTH_ISSUER=https://auth.example.com \
  mcp-shield:latest

# With Docker secrets (production)
docker run -d \
  --name mcp-shield \
  -p 8080:8080 \
  --secret source=jwt-secret,target=/run/secrets/jwt-secret \
  mcp-shield:latest
```

### Health Checks

```bash
# Liveness
curl http://localhost:9091/live

# Readiness
curl http://localhost:9091/ready

# Full health
curl http://localhost:9091/health
```

---

## Docker Compose Deployment

### Production Docker Compose

```yaml
# docker-compose.prod.yml
version: '3.9'

services:
  mcp-shield:
    image: ghcr.io/your-org/mcp-shield:v0.1.0
    restart: unless-stopped
    user: "65532:65532"
    environment:
      - MCP_SHIELD_AUTH_ENABLED=true
      - MCP_SHIELD_AUTH_JWKS_URL=https://auth.example.com/.well-known/jwks.json
      - MCP_SHIELD_TELEMETRY_OTLP_ENDPOINT=http://otel-collector:4317
    ports:
      - "8080:8080"
      - "9090:9090"
    volumes:
      - ./config:/etc/mcp-shield/config:ro
      - ./policies:/etc/mcp-shield/policies:ro
    deploy:
      resources:
        limits:
          cpus: '2'
          memory: 2G
    healthcheck:
      test: ["/usr/local/bin/healthcheck.sh", "health"]
      interval: 30s
      timeout: 10s
      retries: 3
    security_opt:
      - no-new-privileges:true
    cap_drop:
      - ALL
    read_only: true
    tmpfs:
      - /tmp
      - /var/run
```

```bash
# Deploy
docker-compose -f docker-compose.yml -f docker-compose.prod.yml up -d
```

---

## Kubernetes Deployment (Helm)

### Quick Install

```bash
# Add Helm repository (if published)
helm repo add mcp-shield https://charts.mcp-shield.io
helm repo update

# Install with default values
helm install mcp-shield mcp-shield/mcp-shield \
  --namespace mcp-shield \
  --create-namespace
```

### Install from Local Chart

```bash
# Install with custom values
helm install mcp-shield ./helm/mcp-shield \
  --namespace mcp-shield \
  --create-namespace \
  --set image.tag=v0.1.0 \
  --set config.auth.enabled=true \
  --set config.auth.jwks_url=https://auth.example.com/.well-known/jwks.json \
  --set ingress.enabled=true \
  --set ingress.hosts[0].host=mcp-shield.example.com \
  --set ingress.tls[0].secretName=mcp-shield-tls
```

### Using Values Files

```bash
# Production values
helm install mcp-shield ./helm/mcp-shield \
  -f ./helm/mcp-shield/values.yaml \
  -f ./helm/mcp-shield/values-prod.yaml \
  --namespace mcp-shield \
  --create-namespace
```

### Key Configuration Options

| Parameter | Description | Default |
|-----------|-------------|---------|
| `replicaCount` | Number of replicas | `3` |
| `image.repository` | Docker image repository | `ghcr.io/mcp-shield/mcp-shield` |
| `image.tag` | Docker image tag | Chart appVersion |
| `config.auth.enabled` | Enable authentication | `false` |
| `config.auth.jwks_url` | JWKS endpoint URL | `""` |
| `ingress.enabled` | Enable ingress | `true` |
| `ingress.className` | Ingress class | `nginx` |
| `autoscaling.enabled` | Enable HPA | `true` |
| `resources.limits.cpu` | CPU limit | `2000m` |
| `resources.limits.memory` | Memory limit | `2Gi` |

### Cloud Provider Specifics

#### Amazon EKS
```bash
# Enable IRSA for AWS secrets
helm install mcp-shield ./helm/mcp-shield \
  --set serviceAccount.annotations."eks\.amazonaws\.com/role-arn"=arn:aws:iam::123456789012:role/mcp-shield-role
```

#### Google GKE
```bash
# Enable Workload Identity
helm install mcp-shield ./helm/mcp-shield \
  --set serviceAccount.annotations."iam\.gke\.io/service-account"=mcp-shield@my-project.iam.gserviceaccount.com
```

#### Azure AKS
```bash
# Enable Azure AD Workload Identity
helm install mcp-shield ./helm/mcp-shield \
  --set serviceAccount.annotations."azure\.workload\.identity/client-id"=xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
```

#### OpenShift
```bash
# Use SCC for security context constraints
oc adm policy add-scc-to-user restricted -z mcp-shield -n mcp-shield
```

---

## Configuration

### Configuration File Structure

MCP-Shield uses TOML configuration loaded from `/etc/mcp-shield/config/default.toml` with environment variable overrides.

```toml
[server]
bind_addr = "0.0.0.0:8080"
log_level = "info"
json_logging = true
enable_http = true
enable_sse = true
enable_stdio = false
request_timeout_secs = 30
max_request_size = 10485760

[auth]
enabled = true
# jwt_secret = "secret"  # Use secret in production!
jwks_url = "https://auth.example.com/.well-known/jwks.json"
issuer = "https://auth.example.com"
audience = "mcp-shield"
required_scopes = ["mcp:tools:read", "mcp:tools:call"]

[registry]
enforce_prefix_format = true
allowed_prefixes = ["com.company", "io.internal"]

[proxy]
request_timeout_secs = 30
max_concurrent_requests = 100

[telemetry]
metrics_path = "/metrics"
metrics_addr = "0.0.0.0:9090"
otlp_endpoint = "http://otel-collector:4317"
```

### Environment Variable Overrides

All configuration can be overridden via environment variables:

```bash
# Server
MCP_SHIELD_SERVER_BIND_ADDR=0.0.0.0:8080
MCP_SHIELD_SERVER_LOG_LEVEL=info

# Auth
MCP_SHIELD_AUTH_ENABLED=true
MCP_SHIELD_AUTH_JWT_SECRET=secret
MCP_SHIELD_AUTH_JWKS_URL=https://auth.example.com/.well-known/jwks.json
MCP_SHIELD_AUTH_ISSUER=https://auth.example.com

# Registry
MCP_SHIELD_REGISTRY_ENFORCE_PREFIX=true
MCP_SHIELD_REGISTRY_ALLOWED_PREFIXES=com.company,io.internal
```

---

## Secrets Management

### Kubernetes Secrets

```yaml
# Basic secret
apiVersion: v1
kind: Secret
metadata:
  name: mcp-shield-secrets
  namespace: mcp-shield
type: Opaque
stringData:
  jwt-secret: "your-jwt-secret"
  db-password: "your-db-password"
```

### External Secrets Operator (Recommended)

```yaml
# SecretStore for AWS Secrets Manager
apiVersion: external-secrets.io/v1beta1
kind: SecretStore
metadata:
  name: aws-secrets-manager
  namespace: mcp-shield
spec:
  provider:
    aws:
      service: SecretsManager
      region: us-east-1
      auth:
        jwt:
          serviceAccountRef:
            name: mcp-shield

---
# ExternalSecret
apiVersion: external-secrets.io/v1beta1
kind: ExternalSecret
metadata:
  name: mcp-shield-secrets
  namespace: mcp-shield
spec:
  refreshInterval: 1h
  secretStoreRef:
    name: aws-secrets-manager
    kind: SecretStore
  target:
    name: mcp-shield-secrets
    creationPolicy: Owner
  data:
    - secretKey: jwt-secret
      remoteRef:
        key: mcp-shield/production
        property: jwt_secret
    - secretKey: db-password
      remoteRef:
        key: mcp-shield/production
        property: db_password
```

### HashiCorp Vault

```yaml
# VaultSecret
apiVersion: secrets.hashicorp.com/v1beta1
kind: VaultSecret
metadata:
  name: mcp-shield-secrets
  namespace: mcp-shield
spec:
  vaultAuthRef: mcp-shield-auth
  mount: secret
  path: mcp-shield/production
  type: kv-v2
```

---

## TLS/SSL Configuration

### Ingress with cert-manager

```yaml
# Certificate
apiVersion: cert-manager.io/v1
kind: Certificate
metadata:
  name: mcp-shield-tls
  namespace: mcp-shield
spec:
  secretName: mcp-shield-tls
  issuerRef:
    name: letsencrypt-prod
    kind: ClusterIssuer
  dnsNames:
    - mcp-shield.example.com
  privateKey:
    algorithm: RSA
    size: 2048
```

### mTLS Configuration (NGINX Ingress)

```yaml
# Ingress with mTLS
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: mcp-shield
  annotations:
    nginx.ingress.kubernetes.io/auth-tls-verify-client: "on"
    nginx.ingress.kubernetes.io/auth-tls-secret: "mcp-shield/mcp-shield-ca-secret"
    nginx.ingress.kubernetes.io/auth-tls-verify-depth: "1"
    nginx.ingress.kubernetes.io/auth-tls-pass-certificate-to-upstream: "true"
spec:
  tls:
    - hosts:
        - mcp-shield.example.com
      secretName: mcp-shield-tls
  rules:
    - host: mcp-shield.example.com
      http:
        paths:
          - path: /
            pathType: Prefix
            backend:
              service:
                name: mcp-shield
                port:
                  number: 8080
```

---

## Upgrades & Rollbacks

### Helm Upgrade

```bash
# Standard upgrade
helm upgrade mcp-shield ./helm/mcp-shield \
  --namespace mcp-shield \
  --set image.tag=v0.2.0

# Upgrade with values
helm upgrade mcp-shield ./helm/mcp-shield \
  -f values.yaml \
  -f values-prod.yaml \
  --namespace mcp-shield
```

### Canary Deployment

```bash
# Using Argo Rollouts or Flagger
# Label subset of pods for canary
kubectl label pods -l app.kubernetes.io/name=mcp-shield \
  version=canary --overwrite

# Route 10% traffic to canary
# (Requires Istio/Linkerd or NGINX canary annotations)
```

### Rollback

```bash
# Helm rollback
helm rollback mcp-shield --namespace mcp-shield

# Rollback to specific revision
helm rollback mcp-shield 3 --namespace mcp-shield

# View history
helm history mcp-shield --namespace mcp-shield
```

---

## Backup & Disaster Recovery

### Database Backup (PostgreSQL)

```bash
# Backup
kubectl exec -n mcp-shield postgresql-0 -- \
  pg_dump -U mcp_shield mcp_shield > backup-$(date +%Y%m%d).sql

# Restore
kubectl exec -i -n mcp-shield postgresql-0 -- \
  psql -U mcp_shield mcp_shield < backup-20240101.sql
```

### Redis Backup

```bash
# Backup
kubectl exec -n mcp-shield redis-0 -- \
  redis-cli -a $REDIS_PASSWORD BGSAVE
kubectl cp mcp-shield/redis-0:/data/dump.rdb ./redis-backup-$(date +%Y%m%d).rdb

# Restore
kubectl cp ./redis-backup.rdb mcp-shield/redis-0:/data/dump.rdb
kubectl exec -n mcp-shield redis-0 -- redis-cli -a $REDIS_PASSWORD SHUTDOWN NOSAVE
```

### Velero Backup (Full Cluster)

```bash
# Install Velero
velero install \
  --provider aws \
  --plugins velero/velero-plugin-for-aws:v1.8.0 \
  --bucket mcp-shield-backups \
  --backup-location-config region=us-east-1 \
  --snapshot-location-config region=us-east-1

# Backup namespace
velero backup create mcp-shield-backup-$(date +%Y%m%d) \
  --include-namespaces mcp-shield \
  --wait

# Restore
velero restore create --from-backup mcp-shield-backup-20240101
```

---

## Scaling

### Horizontal Pod Autoscaler (HPA)

```yaml
# Configured in values.yaml
autoscaling:
  enabled: true
  minReplicas: 3
  maxReplicas: 20
  targetCPUUtilizationPercentage: 70
  targetMemoryUtilizationPercentage: 80
```

### Manual Scaling

```bash
# Scale manually
kubectl scale deployment mcp-shield --replicas=10 -n mcp-shield

# Or via Helm
helm upgrade mcp-shield ./helm/mcp-shield \
  --set replicaCount=10 \
  --namespace mcp-shield
```

### Cluster Autoscaler

Ensure cluster autoscaler is configured for your cloud provider:
- **AWS**: Cluster Autoscaler with ASG tags
- **GKE**: Enable GKE Autopilot or Cluster Autoscaler
- **AKS**: Enable Cluster Autoscaler

---

## Monitoring & Observability

### Prometheus Metrics

Key metrics exposed at `/metrics`:

| Metric | Type | Description |
|--------|------|-------------|
| `mcp_requests_total` | Counter | Total requests by method, transport, status |
| `mcp_request_duration_seconds` | Histogram | Request latency percentiles |
| `mcp_active_sessions` | Gauge | Current active sessions |
| `mcp_auth_failures_total` | Counter | Authentication failures by reason |
| `mcp_validation_failures_total` | Counter | Validation failures by reason |
| `mcp_blocked_requests_total` | Counter | Blocked requests by reason |
| `mcp_policy_evaluation_duration_seconds` | Histogram | Policy evaluation time |
| `mcp_upstream_requests_total` | Counter | Upstream requests by server, status |

### Grafana Dashboards

Pre-built dashboards available in `docker/grafana/dashboards/mcp-shield/`:
- **MCP-Shield Overview** - Request rates, latency, errors
- **Authentication** - Auth success/failure rates, token validation
- **Policy Engine** - Policy evaluation latency, decisions
- **Upstream Health** - Upstream latency, circuit breaker status
- **Resources** - CPU, memory, network, disk

### Distributed Tracing (Jaeger)

```bash
# Access Jaeger UI
kubectl port-forward -n observability svc/jaeger 16686:16686
# Open http://localhost:16686
```

### Log Aggregation (Loki)

```bash
# Query logs via Loki
curl -G -s "http://loki:3100/loki/api/v1/query_range" \
  --data-urlencode 'query={job="mcp-shield"} |~ "error"' \
  --data-urlencode 'limit=100'
```

---

## Troubleshooting

### Common Issues

#### Pod Not Starting

```bash
# Check pod status
kubectl describe pod -l app.kubernetes.io/name=mcp-shield -n mcp-shield

# Check logs
kubectl logs -l app.kubernetes.io/name=mcp-shield -n mcp-shield --tail=100

# Check events
kubectl get events -n mcp-shield --sort-by='.lastTimestamp'
```

#### Configuration Issues

```bash
# View rendered config
kubectl exec -n mcp-shield deploy/mcp-shield -- cat /etc/mcp-shield/config/default.toml

# Check configmap
kubectl get configmap mcp-shield-config -n mcp-shield -o yaml
```

#### Authentication Failures

```bash
# Check auth config
kubectl logs -l app.kubernetes.io/name=mcp-shield -n mcp-shield | grep -i auth

# Verify JWKS endpoint
curl https://auth.example.com/.well-known/jwks.json
```

#### High Latency

```bash
# Check metrics
kubectl port-forward -n mcp-shield svc/mcp-shield 9090:9090
# Query Prometheus: histogram_quantile(0.99, rate(mcp_request_duration_seconds_bucket[5m]))
```

#### Upstream Connection Issues

```bash
# Check upstream connectivity
kubectl exec -n mcp-shield deploy/mcp-shield -- curl -v http://upstream:8080/health

# Check circuit breaker status
curl http://localhost:9090/metrics | grep circuit_breaker
```

### Debug Mode

```bash
# Enable debug logging
kubectl set env deployment/mcp-shield -n mcp-shield RUST_LOG=debug

# Or via Helm
helm upgrade mcp-shield ./helm/mcp-shield \
  --set config.server.log_level=debug \
  --namespace mcp-shield
```

---

## Production Hardening

### Security Checklist

- [ ] **Run as non-root** (UID 65532)
- [ ] **Read-only root filesystem**
- [ ] **Drop all capabilities**
- [ ] **Seccomp profile** (RuntimeDefault)
- [ ] **Network policies** (ingress/egress)
- [ ] **Pod Security Standards** (restricted)
- [ ] **Image signing** (Cosign/Sigstore)
- [ ] **SBOM generation** (Syft)
- [ ] **Vulnerability scanning** (Trivy, Grype)
- [ ] **Secret management** (External Secrets, Vault)
- [ ] **TLS everywhere** (ingress, mTLS)
- [ ] **Resource limits** (CPU, memory)
- [ ] **PodDisruptionBudget** configured
- [ ] **Anti-affinity** rules for HA
- [ ] **Topology spread** across zones

### Network Policies

```yaml
# Default deny all (apply to namespace)
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: default-deny
spec:
  podSelector: {}
  policyTypes:
    - Ingress
    - Egress
```

### Pod Security Standards (Kyverno)

```yaml
# Enforce restricted PSP via Kyverno
apiVersion: kyverno.io/v1
kind: ClusterPolicy
metadata:
  name: restricted-psp
spec:
  validationFailureAction: Enforce
  rules:
    - name: require-non-root
      match:
        any:
          - resources:
              kinds: ["Pod"]
      validate:
        pattern:
          spec:
            securityContext:
              runAsNonRoot: true
```

### Image Verification

```bash
# Verify image signature
cosign verify ghcr.io/mcp-shield/mcp-shield:v0.1.0 \
  --certificate-identity-regexp=".*" \
  --certificate-oidc-issuer-regexp=".*"

# Verify SBOM
syft packages ghcr.io/mcp-shield/mcp-shield:v0.1.0 -o table
```

---

## Support

- **Documentation**: https://github.com/mcp-shield/mcp-shield/docs
- **Issues**: https://github.com/mcp-shield/mcp-shield/issues
- **Discussions**: https://github.com/mcp-shield/mcp-shield/discussions
- **Security**: security@mcp-shield.io
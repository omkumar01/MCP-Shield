# ┌──────────────────────────────────────────────────────────────────────────────┐
# │ MCP-Shield Docker Compose Documentation                                      │
# │                                                                              │
# │ Local development and testing with full observability stack.                │
# └──────────────────────────────────────────────────────────────────────────────┘

---

## Quick Start

```bash
# Start all services
docker-compose -f docker-compose.yml -f docker-compose.dev.yml up -d

# View logs
docker-compose -f docker-compose.yml -f docker-compose.dev.yml logs -f

# Stop and cleanup
docker-compose -f docker-compose.yml -f docker-compose.dev.yml down -v
```

---

## Service Overview

| Service | Port | Description |
|---------|------|-------------|
| **mcp-shield** | 8080 | MCP Security Gateway |
| | 9090 | Prometheus Metrics |
| | 9091 | Health Checks |
| **mcp-echo** | 8081 | Echo Test Server |
| **postgres** | 5432 | PostgreSQL Database |
| **redis** | 6379 | Redis Cache |
| **otel-collector** | 4317 | OTLP gRPC |
| | 4318 | OTLP HTTP |
| | 8888 | Prometheus Metrics |
| **prometheus** | 9090 | Metrics Collection |
| **grafana** | 3000 | Dashboards (admin/admin) |
| **loki** | 3100 | Log Aggregation |
| **promtail** | 9080 | Log Shipper |
| **jaeger** | 16686 | Tracing UI |
| | 6831/6832 | Jaeger Thrift |
| | 14268 | Jaeger HTTP |
| | 4317/4318 | OTLP |

---

## Access Points

### MCP Gateway
```bash
# Health
curl http://localhost:9091/health

# Metrics
curl http://localhost:9090/metrics

# MCP API
curl -X POST http://localhost:8080/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}'
```

### Observability Stack
| Service | URL | Credentials |
|---------|-----|-------------|
| **Grafana** | http://localhost:3000 | admin / admin |
| **Prometheus** | http://localhost:9090 | - |
| **Jaeger** | http://localhost:16686 | - |
| **Loki** | http://localhost:3100 | - |

### Development Tools (with `--profile dev-tools`)
| Service | URL | Credentials |
|---------|-----|-------------|
| **Mailhog** | http://localhost:8025 | - |
| **Redis Commander** | http://localhost:8082 | - |
| **pgAdmin** | http://localhost:5050 | admin@local.dev / admin |

---

## Configuration

### Environment Variables
Copy `.env.example` to `.env` and customize:

```bash
cp .env.example .env
# Edit .env with your values
```

Key variables:
- `MCP_SHIELD_LOG_LEVEL` - Log level (debug, info, warn, error)
- `MCP_SHIELD_AUTH_ENABLED` - Enable authentication
- `POSTGRES_PASSWORD` - Database password
- `REDIS_PASSWORD` - Redis password
- `GRAFANA_ADMIN_PASSWORD` - Grafana admin password

### Custom Config
Mount your config files:
```yaml
# In docker-compose.override.yml
services:
  mcp-shield:
    volumes:
      - ./my-config:/etc/mcp-shield/config:ro
      - ./my-policies:/etc/mcp-shield/policies:ro
```

---

## Profiles

```bash
# Core services only
docker-compose up -d

# With development tools
docker-compose --profile dev-tools up -d

# With all tools
docker-compose --profile dev-tools up -d
```

---

## Volumes (Persistent Data)

| Volume | Service | Description |
|--------|---------|-------------|
| `mcp-shield-postgres-data` | postgres | Database files |
| `mcp-shield-redis-data` | redis | Redis persistence |
| `mcp-shield-prometheus-data` | prometheus | Metrics storage |
| `mcp-shield-grafana-data` | grafana | Dashboards, config |
| `mcp-shield-loki-data` | loki | Log storage |
| `mcp-shield-jaeger-data` | jaeger | Trace storage |
| `mcp-shield-config` | mcp-shield | Configuration |
| `mcp-shield-policies` | mcp-shield | Cedar policies |

---

## Networks

| Network | Subnet | Services |
|---------|--------|----------|
| `mcp-shield-frontend` | 172.28.0.0/24 | mcp-shield, mcp-echo |
| `mcp-shield-backend` | 172.28.1.0/24 | postgres, redis, mcp-shield |
| `mcp-shield-observability` | 172.28.2.0/24 | All observability services |

---

## Health Checks

All services have health checks configured:

```bash
# Check all health statuses
docker-compose ps

# Detailed health
docker inspect mcp-shield-gateway | jq '.[0].State.Health'
```

---

## Troubleshooting

### Service Won't Start
```bash
# Check logs
docker-compose logs <service-name>

# Check resource usage
docker stats

# Check port conflicts
netstat -tulpn | grep -E '8080|9090|3000|9090'
```

### Database Connection Issues
```bash
# Test postgres
docker-compose exec postgres pg_isready -U mcp_shield -d mcp_shield

# Test from gateway
docker-compose exec mcp-shield psql postgresql://mcp_shield:changeme@postgres:5432/mcp_shield
```

### Redis Connection Issues
```bash
# Test redis
docker-compose exec redis redis-cli -a changeme ping

# Check memory
docker-compose exec redis redis-cli -a changeme info memory
```

### Metrics Not Appearing
```bash
# Check prometheus targets
curl http://localhost:9090/api/v1/targets

# Check scrape config
docker-compose exec prometheus cat /etc/prometheus/prometheus.yml
```

### Logs Not in Loki
```bash
# Check promtail
docker-compose logs promtail

# Query Loki directly
curl -G -s "http://localhost:3100/loki/api/v1/query_range" \
  --data-urlencode 'query={job="docker-logs"}' \
  --data-urlencode 'limit=10'
```

---

## Resource Requirements

### Minimum (Development)
- **CPU**: 2 cores
- **Memory**: 4 GB
- **Disk**: 10 GB

### Recommended (Full Stack)
- **CPU**: 4 cores
- **Memory**: 8 GB
- **Disk**: 20 GB

### Production-Like
- **CPU**: 8 cores
- **Memory**: 16 GB
- **Disk**: 50 GB

Adjust in `docker-compose.yml` under `deploy.resources`.

---

## Customization

### Add Custom Service
```yaml
# docker-compose.override.yml
services:
  my-service:
    image: my-image:latest
    networks:
      - mcp-shield-backend
    environment:
      - MCP_SHIELD_UPSTREAM_URL=http://my-service:8080
```

### Modify Resources
```yaml
# docker-compose.override.yml
services:
  mcp-shield:
    deploy:
      resources:
        limits:
          cpus: '4'
          memory: 4G
        reservations:
          cpus: '1'
          memory: 1G
```

### Custom Grafana Dashboards
```yaml
# docker-compose.override.yml
services:
  grafana:
    volumes:
      - ./my-dashboards:/var/lib/grafana/dashboards/my-dashboards:ro
```

---

## Cleanup

```bash
# Stop and remove containers
docker-compose down

# Stop and remove containers + volumes (data loss!)
docker-compose down -v

# Remove all images
docker-compose down -v --rmi all

# Clean Docker system
docker system prune -af --volumes
```

---

## CI/CD Integration

### GitHub Actions
```yaml
- name: Start Docker Compose
  run: |
    docker-compose -f docker-compose.yml -f docker-compose.dev.yml up -d
    sleep 30  # Wait for services
    
- name: Run Tests
  run: |
    cargo test --test integration
    
- name: Stop Docker Compose
  if: always()
  run: docker-compose -f docker-compose.yml -f docker-compose.dev.yml down -v
```

---

## Production vs Development

| Aspect | Development | Production |
|--------|-------------|------------|
| **Auth** | Disabled | Enabled (JWT/JWKS) |
| **Log Level** | debug | info |
| **Persistence** | tmpfs (ephemeral) | Persistent volumes |
| **Resources** | Minimal | Sized for load |
| **Replicas** | 1 | 3+ with HPA |
| **TLS** | Self-signed | Valid certificates |
| **Secrets** | .env file | External secrets |

---

## Support

- **Issues**: https://github.com/omkumar01/mcp-shield/issues
- **Docs**: https://github.com/omkumar01/mcp-shield/docs
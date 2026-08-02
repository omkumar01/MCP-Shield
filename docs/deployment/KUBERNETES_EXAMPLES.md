# ┌──────────────────────────────────────────────────────────────────────────────┐
# │ MCP-Shield Kubernetes Deployment Examples                                     │
# │                                                                              │
# │ Ready-to-use Kubernetes manifests for various deployment scenarios.          │
# └──────────────────────────────────────────────────────────────────────────────┘

---

## Quick Start: Helm (Recommended)

```bash
# Install with production values
helm install mcp-shield ./helm/mcp-shield \
  --namespace mcp-shield \
  --create-namespace \
  -f ./helm/mcp-shield/values-prod.yaml
```

---

## Manual Kubernetes Manifests

### Namespace & RBAC

```yaml
# k8s/namespace.yaml
apiVersion: v1
kind: Namespace
metadata:
  name: mcp-shield
  labels:
    name: mcp-shield
    environment: production
---
# k8s/rbac.yaml
apiVersion: v1
kind: ServiceAccount
metadata:
  name: mcp-shield
  namespace: mcp-shield
  annotations:
    eks.amazonaws.com/role-arn: arn:aws:iam::123456789012:role/mcp-shield
---
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: mcp-shield
  namespace: mcp-shield
rules:
  - apiGroups: [""]
    resources: ["configmaps", "secrets", "pods", "services", "endpoints"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["networking.k8s.io"]
    resources: ["ingresses"]
    verbs: ["get", "list", "watch", "create", "update", "patch"]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: mcp-shield
  namespace: mcp-shield
subjects:
  - kind: ServiceAccount
    name: mcp-shield
    namespace: mcp-shield
roleRef:
  kind: Role
  name: mcp-shield
  apiGroup: rbac.authorization.k8s.io
```

### ConfigMap

```yaml
# k8s/configmap.yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: mcp-shield-config
  namespace: mcp-shield
data:
  default.toml: |
    [server]
    bind_addr = "0.0.0.0:8080"
    log_level = "info"
    json_logging = true
    enable_http = true
    enable_sse = true
    enable_stdio = false
    request_timeout_secs = 30
    
    [auth]
    enabled = true
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
    upstream_tls_verify = true
    
    [telemetry]
    metrics_path = "/metrics"
    metrics_addr = "0.0.0.0:9090"
    health_addr = "0.0.0.0:9091"
    otlp_endpoint = "https://otel-collector.observability.svc.cluster.local:4317"
```

### Secret (Use External Secrets in Production)

```yaml
# k8s/secret.yaml
apiVersion: v1
kind: Secret
metadata:
  name: mcp-shield-secrets
  namespace: mcp-shield
type: Opaque
stringData:
  # In production, use External Secrets Operator, Vault, or cloud provider secrets
  # This is for demonstration only
  jwt-secret: "your-32-char-minimum-secret-key"
  db-password: "your-db-password"
  redis-password: "your-redis-password"
  otel-token: "your-otel-token"
```

### Deployment

```yaml
# k8s/deployment.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: mcp-shield
  namespace: mcp-shield
  labels:
    app.kubernetes.io/name: mcp-shield
    app.kubernetes.io/version: "0.1.0"
    app.kubernetes.io/component: gateway
    app.kubernetes.io/managed-by: helm
spec:
  replicas: 3
  selector:
    matchLabels:
      app.kubernetes.io/name: mcp-shield
  strategy:
    type: RollingUpdate
    rollingUpdate:
      maxSurge: 25%
      maxUnavailable: 25%
  template:
    metadata:
      labels:
        app.kubernetes.io/name: mcp-shield
        app.kubernetes.io/version: "0.1.0"
        app.kubernetes.io/component: gateway
      annotations:
        prometheus.io/scrape: "true"
        prometheus.io/port: "9090"
        prometheus.io/path: "/metrics"
    spec:
      serviceAccountName: mcp-shield
      securityContext:
        runAsNonRoot: true
        runAsUser: 65532
        runAsGroup: 65532
        fsGroup: 65532
        fsGroupChangePolicy: OnRootMismatch
        seccompProfile:
          type: RuntimeDefault
      affinity:
        podAntiAffinity:
          preferredDuringSchedulingIgnoredDuringExecution:
            - weight: 100
              podAffinityTerm:
                labelSelector:
                  matchExpressions:
                    - key: app.kubernetes.io/name
                      operator: In
                      values:
                        - mcp-shield
                topologyKey: topology.kubernetes.io/zone
            - weight: 50
              podAffinityTerm:
                labelSelector:
                  matchExpressions:
                    - key: app.kubernetes.io/name
                      operator: In
                      values:
                        - mcp-shield
                topologyKey: kubernetes.io/hostname
      topologySpreadConstraints:
        - maxSkew: 1
          topologyKey: topology.kubernetes.io/zone
          whenUnsatisfiable: ScheduleAnyway
          labelSelector:
            matchLabels:
              app.kubernetes.io/name: mcp-shield
        - maxSkew: 1
          topologyKey: kubernetes.io/hostname
          whenUnsatisfiable: ScheduleAnyway
          labelSelector:
            matchLabels:
              app.kubernetes.io/name: mcp-shield
      containers:
        - name: mcp-shield
          image: ghcr.io/omkumar01/mcp-shield:v0.1.0
          imagePullPolicy: IfNotPresent
          securityContext:
            allowPrivilegeEscalation: false
            capabilities:
              drop:
                - ALL
            readOnlyRootFilesystem: true
            runAsNonRoot: true
            runAsUser: 65532
            runAsGroup: 65532
            seccompProfile:
              type: RuntimeDefault
          args:
            - --config
            - /etc/mcp-shield/config/default.toml
          ports:
            - name: http
              containerPort: 8080
              protocol: TCP
            - name: metrics
              containerPort: 9090
              protocol: TCP
            - name: health
              containerPort: 9091
              protocol: TCP
          env:
            - name: MCP_SHIELD_CONFIG
              value: /etc/mcp-shield/config/default.toml
            - name: RUST_LOG
              value: info
            - name: OTEL_SERVICE_NAME
              value: mcp-shield
          envFrom:
            - configMapRef:
                name: mcp-shield-config
            - secretRef:
                name: mcp-shield-secrets
          resources:
            limits:
              cpu: "2000m"
              memory: "2Gi"
            requests:
              cpu: "500m"
              memory: "512Mi"
          livenessProbe:
            httpGet:
              path: /healthz
              port: health
            initialDelaySeconds: 30
            periodSeconds: 30
            timeoutSeconds: 10
            failureThreshold: 3
          readinessProbe:
            httpGet:
              path: /readyz
              port: health
            initialDelaySeconds: 10
            periodSeconds: 10
            timeoutSeconds: 5
            failureThreshold: 3
          startupProbe:
            httpGet:
              path: /livez
              port: health
            initialDelaySeconds: 10
            periodSeconds: 10
            timeoutSeconds: 5
            failureThreshold: 30
          volumeMounts:
            - name: config
              mountPath: /etc/mcp-shield/config
              readOnly: true
            - name: tmp
              mountPath: /tmp
            - name: data
              mountPath: /var/lib/mcp-shield/data
      volumes:
        - name: config
          configMap:
            name: mcp-shield-config
            items:
              - key: default.toml
                path: default.toml
        - name: tmp
          emptyDir:
            sizeLimit: 100Mi
        - name: data
          emptyDir:
            sizeLimit: 500Mi
```

### Service

```yaml
# k8s/service.yaml
apiVersion: v1
kind: Service
metadata:
  name: mcp-shield
  namespace: mcp-shield
  labels:
    app.kubernetes.io/name: mcp-shield
  annotations:
    prometheus.io/scrape: "true"
    prometheus.io/port: "9090"
    prometheus.io/path: "/metrics"
spec:
  type: ClusterIP
  ports:
    - name: http
      port: 8080
      targetPort: http
      protocol: TCP
      appProtocol: http
    - name: metrics
      port: 9090
      targetPort: metrics
      protocol: TCP
      appProtocol: http
  selector:
    app.kubernetes.io/name: mcp-shield
```

### Ingress (NGINX)

```yaml
# k8s/ingress.yaml
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: mcp-shield
  namespace: mcp-shield
  annotations:
    nginx.ingress.kubernetes.io/ssl-redirect: "true"
    nginx.ingress.kubernetes.io/proxy-body-size: "10m"
    nginx.ingress.kubernetes.io/proxy-read-timeout: "120"
    nginx.ingress.kubernetes.io/proxy-send-timeout: "120"
    nginx.ingress.kubernetes.io/rate-limit: "1000"
    nginx.ingress.kubernetes.io/rate-limit-window: "1s"
    # mTLS (optional)
    # nginx.ingress.kubernetes.io/auth-tls-verify-client: "on"
    # nginx.ingress.kubernetes.io/auth-tls-secret: "mcp-shield/mcp-shield-ca-secret"
    # nginx.ingress.kubernetes.io/auth-tls-verify-depth: "1"
    # nginx.ingress.kubernetes.io/auth-tls-pass-certificate-to-upstream: "true"
    # Security headers
    nginx.ingress.kubernetes.io/configuration-snippet: |
      add_header X-Content-Type-Options nosniff;
      add_header X-Frame-Options DENY;
      add_header X-XSS-Protection "1; mode=block";
      add_header Strict-Transport-Security "max-age=31536000; includeSubDomains" always;
spec:
  ingressClassName: nginx
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

### NetworkPolicy

```yaml
# k8s/networkpolicy.yaml
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: mcp-shield-ingress
  namespace: mcp-shield
spec:
  podSelector:
    matchLabels:
      app.kubernetes.io/name: mcp-shield
  policyTypes:
    - Ingress
  ingress:
    - from:
        - namespaceSelector:
            matchLabels:
              name: ingress-nginx
        - namespaceSelector:
            matchLabels:
              name: monitoring
        - podSelector:
            matchLabels:
              app.kubernetes.io/name: mcp-shield
      ports:
        - protocol: TCP
          port: 8080
        - protocol: TCP
          port: 9090
        - protocol: TCP
          port: 9091
---
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: mcp-shield-egress
  namespace: mcp-shield
spec:
  podSelector:
    matchLabels:
      app.kubernetes.io/name: mcp-shield
  policyTypes:
    - Egress
  egress:
    - to:
        - namespaceSelector:
            matchLabels:
              name: kube-system
      ports:
        - protocol: TCP
          port: 53
        - protocol: UDP
          port: 53
    - to:
        - namespaceSelector:
            matchLabels:
              name: database
      ports:
        - protocol: TCP
          port: 5432
    - to:
        - namespaceSelector:
            matchLabels:
              name: cache
      ports:
        - protocol: TCP
          port: 6379
    - to:
        - namespaceSelector:
            matchLabels:
              name: messaging
      ports:
        - protocol: TCP
          port: 9092
    - to:
        - namespaceSelector:
            matchLabels:
              name: observability
      ports:
        - protocol: TCP
          port: 4317
        - protocol: TCP
          port: 3100
        - protocol: TCP
          port: 14268
```

### HPA

```yaml
# k8s/hpa.yaml
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: mcp-shield
  namespace: mcp-shield
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: mcp-shield
  minReplicas: 3
  maxReplicas: 20
  metrics:
    - type: Resource
      resource:
        name: cpu
        target:
          type: Utilization
          averageUtilization: 70
    - type: Resource
      resource:
        name: memory
        target:
          type: Utilization
          averageUtilization: 80
  behavior:
    scaleDown:
      stabilizationWindowSeconds: 300
      policies:
        - type: Percent
          value: 10
          periodSeconds: 60
    scaleUp:
      stabilizationWindowSeconds: 0
      policies:
        - type: Percent
          value: 100
          periodSeconds: 15
        - type: Pods
          value: 4
          periodSeconds: 15
      selectPolicy: Max
```

### PodDisruptionBudget

```yaml
# k8s/pdb.yaml
apiVersion: policy/v1
kind: PodDisruptionBudget
metadata:
  name: mcp-shield
  namespace: mcp-shield
spec:
  minAvailable: 50%
  selector:
    matchLabels:
      app.kubernetes.io/name: mcp-shield
```

### ServiceMonitor (Prometheus Operator)

```yaml
# k8s/servicemonitor.yaml
apiVersion: monitoring.coreos.com/v1
kind: ServiceMonitor
metadata:
  name: mcp-shield
  namespace: monitoring
  labels:
    release: prometheus
spec:
  selector:
    matchLabels:
      app.kubernetes.io/name: mcp-shield
  endpoints:
    - port: metrics
      path: /metrics
      interval: 30s
      scrapeTimeout: 10s
  namespaceSelector:
    matchNames:
      - mcp-shield
```

---

## Kustomize Overlay Structure

```
k8s/
├── base/
│   ├── kustomization.yaml
│   ├── namespace.yaml
│   ├── rbac.yaml
│   ├── configmap.yaml
│   ├── secret.yaml
│   ├── deployment.yaml
│   ├── service.yaml
│   ├── ingress.yaml
│   ├── networkpolicy.yaml
│   ├── hpa.yaml
│   ├── pdb.yaml
│   └── servicemonitor.yaml
├── overlays/
│   ├── development/
│   │   ├── kustomization.yaml
│   │   └── patches/
│   │       ├── replica-count.yaml
│   │       └── resources.yaml
│   ├── staging/
│   │   ├── kustomization.yaml
│   │   └── patches/
│   │       ├── replica-count.yaml
│   │       └── resources.yaml
│   └── production/
│       ├── kustomization.yaml
│       └── patches/
│           ├── replica-count.yaml
│           ├── resources.yaml
│           ├── image-digest.yaml
│           └── security-context.yaml
```

### Production Kustomization

```yaml
# k8s/overlays/production/kustomization.yaml
apiVersion: kustomize.config.k8s.io/v1beta1
kind: Kustomization

namespace: mcp-shield

resources:
  - ../../base

patchesStrategicMerge:
  - patches/replica-count.yaml
  - patches/resources.yaml
  - patches/image-digest.yaml
  - patches/security-context.yaml

commonLabels:
  environment: production
  team: platform

images:
  - name: ghcr.io/omkumar01/mcp-shield
    newTag: v0.1.0
    digest: sha256:abc123...
```

```yaml
# k8s/overlays/production/patches/replica-count.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: mcp-shield
spec:
  replicas: 5
```

```yaml
# k8s/overlays/production/patches/resources.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: mcp-shield
spec:
  template:
    spec:
      containers:
        - name: mcp-shield
          resources:
            limits:
              cpu: "4000m"
              memory: "4Gi"
            requests:
              cpu: "1000m"
              memory: "1Gi"
```

```yaml
# k8s/overlays/production/patches/image-digest.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: mcp-shield
spec:
  template:
    spec:
      containers:
        - name: mcp-shield
          image: ghcr.io/omkumar01/mcp-shield@sha256:abc123def456...
```

```yaml
# k8s/overlays/production/patches/security-context.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: mcp-shield
spec:
  template:
    spec:
      securityContext:
        runAsNonRoot: true
        runAsUser: 65532
        runAsGroup: 65532
        fsGroup: 65532
        seccompProfile:
          type: RuntimeDefault
      containers:
        - name: mcp-shield
          securityContext:
            allowPrivilegeEscalation: false
            capabilities:
              drop:
                - ALL
            readOnlyRootFilesystem: true
            runAsNonRoot: true
            runAsUser: 65532
            runAsGroup: 65532
            seccompProfile:
              type: RuntimeDefault
```

---

## Apply with Kustomize

```bash
# Development
kubectl apply -k k8s/overlays/development

# Staging
kubectl apply -k k8s/overlays/staging

# Production
kubectl apply -k k8s/overlays/production
```

---

## Verify Deployment

```bash
# Check all resources
kubectl get all -n mcp-shield

# Check pod status
kubectl get pods -n mcp-shield -o wide

# Check logs
kubectl logs -n mcp-shield -l app.kubernetes.io/name=mcp-shield -f

# Check metrics
kubectl port-forward -n mcp-shield svc/mcp-shield 9090:9090
curl localhost:9090/metrics

# Check health
kubectl port-forward -n mcp-shield svc/mcp-shield 9091:9091
curl localhost:9091/health
curl localhost:9091/ready
```
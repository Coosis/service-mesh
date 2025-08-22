# Service-Mesh
## Side Car Proxy
- Runs alongside main application
- Reverse proxy for incoming requests
- Request retries with exponential backoff
- Load balancing
- Health checks
- Passive health (mark on consecutive 5xx/timeouts) + active /healthz
- Circuit breaking & outlier ejection (protects backends when they degrade)
- Per-route timeouts (prevent request pile-ups)
- LB policy: start with round-robin, add P2C (peak-two-choices) least-loaded
- Admin port (e.g., :15000) to expose /metrics, /ready, /config
- Tracing propagation (traceparent / B3) even before full OTEL
- Graceful drain on shutdown (stop accepting, finish in-flight)
- Consistent hashing for sticky sessions
- Request shadowing/mirroring (copy a % to canary)
## Control Plane
- Access control list
- mTLS
- Routing rules to Side Cars
- Telemetry configuration
- Service discovery source (Kubernetes Endpoints/EndpointSlice, Consul, or etcd prefix). The CP watches it and pushes instance lists to sidecars.
- Config distribution model: pull (sidecars poll CP) or push (server-streaming). Aim for server-streaming gRPC (xDS-style) but start with polling.
- Identity: issue short-lived certs (SPIFFE-like IDs) from a simple CA; rotate automatically.
- Rollout policies: weighted routing (canary 10/90), header-based A/B, failover.
- Policy evaluation order: authZ/ACL → routing → retries/timeouts → telemetry.
- Versioned configs with generation numbers; ACK/NACK from sidecars.
- Central dashboard (CP aggregates sidecar heartbeats/metrics metadata).

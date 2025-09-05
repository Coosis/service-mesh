# Service-Mesh
## Side Car Proxy
[X] Runs alongside main application
[X] Reverse proxy for incoming requests
[X] Load balancing(round-robin/p2c with least-loaded policy, tie break using latency)
[X] Active Health (periodic /healthz checks)
[X] Passive health (mark on consecutive 5xx/timeouts)
[ ] Circuit breaking
[X] Outlier ejection (protects backends when they degrade)
[X] Per-route timeouts (prevent request pile-ups)
[ ] Admin port (e.g., :15000) to expose /metrics, /ready, /config
[X] Tracing propagation (W3C traceparent) before full OTEL
[X] Graceful drain on shutdown (stop accepting, finish in-flight)
[ ] Consistent hashing for sticky sessions
[ ] Request shadowing/mirroring (copy a % to canary)
[ ] TLS termination
## Control Plane
[ ] Access control list
[ ] mTLS
[ ] Routing rules to Side Cars
[ ] Telemetry configuration
[ ] Service discovery source (Kubernetes Endpoints/EndpointSlice, Consul, or etcd prefix). The CP watches it and pushes instance lists to sidecars.
[ ] Config distribution model: pull (sidecars poll CP) or push (server-streaming). Aim for server-streaming gRPC (xDS-style) but start with polling.
[ ] Identity: issue short-lived certs (SPIFFE-like IDs) from a simple CA; rotate automatically.
[ ] Rollout policies: weighted routing (canary 10/90), header-based A/B, failover.
[ ] Policy evaluation order: authZ/ACL → routing → retries/timeouts → telemetry.
[ ] Versioned configs with generation numbers; ACK/NACK from sidecars.
[ ] Central dashboard (CP aggregates sidecar heartbeats/metrics metadata).

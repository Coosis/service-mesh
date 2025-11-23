# Service-Mesh
This repository contains a simple implementation of a service mesh architecture, 
including sidecar proxies and a control plane. There's also a basic demo application for 
testing purposes.

They are `control-plane/`, `side-car-proxy/`, and `test-service` respectively.

## Getting started

### Control Plane
```bash
cargo run -p control-plane
```
Upload new config(example uses `curl`):
```bash
curl -X POST http://localhost:13000/upload_config \
     --data-binary "@example/proxy_config.toml" \
     -H "Content-Type: application/octet-stream"
```

### Side Car Proxy

Note: example uses mtls by default, if you don't want tls, 
checkout the `example/proxy_config.toml` and modify the `tls` section accordingly.
If you've already started the control plane, either restart it or upload the altered config, 
then jump to `step 2`.

1. Generate self-signed certs for service-a and service-b
This requires `openssl`.
```bash
openssl genpkey \
  -algorithm RSA \
  -pkeyopt rsa_keygen_bits:4096 \
  -out rootCA.key

openssl req \
  -x509 \
  -new \
  -key rootCA.key \
  -sha256 \
  -days 3650 \
  -out rootCA.crt

./gen_cert.sh service-a.cluster.local
./gen_cert.sh service-b.cluster.local
./gen_client_cert.sh service-a.cluster.local
./gen_client_cert.sh service-b.cluster.local
```

2. start cluster a
```bash
export export SERVICE_NAME="service-a"
cargo run -p side-car-proxy
```

3. start cluster b
```bash
export export SERVICE_NAME="service-b"
cargo run -p side-car-proxy
```

4. start a test service for service-a
```bash
cargo run -p test-service -- 8314
```

5. start a test service for service-b
```bash
cargo run -p test-service -- 8317
```

6. Testing out mTLS
```bash
curl -X GET http://localhost:8317/call/localhost%3A8435/localhost%3A8533/internal%2Fok
```
The first part `http://localhost:8317` is the service-b test service endpoint.
`/call/localhost%3A8435/localhost%3A8533/internal%2Fok` means service-b sidecar will forward 
the request using its service-b sidecar at `localhost:8435`, service-b sidecar establishes 
mTLS connection to service-a sidecar at (`localhost:8533`), which in turn forwards to 
service-a test service with path parameter `/internal/ok`. 

To also test the ACL feature, you can directly call service-a sidecar without using mTLS:
```bash
curl -X GET https://localhost:8333/internal/ok -k
```


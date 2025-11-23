#!/usr/bin/env bash
set -euo pipefail

# === Config ===
# Override these via env if needed:
#   CA_CERT=rootCA.crt CA_KEY=rootCA.key ./gen_server_cert.sh ...
CA_CERT="${CA_CERT:-rootCA.crt}"
CA_KEY="${CA_KEY:-rootCA.key}"
DAYS="${DAYS:-3650}"

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 <server_name> [out_dir]"
  echo "Example: $0 localhost certs/localhost"
  exit 1
fi

SERVER_NAME="$1"
OUT_DIR="${2:-certs/$SERVER_NAME}"

if [[ ! -f "$CA_CERT" || ! -f "$CA_KEY" ]]; then
  echo "ERROR: CA files not found."
  echo "  Expected:"
  echo "    CA_CERT=$CA_CERT"
  echo "    CA_KEY=$CA_KEY"
  exit 1
fi

mkdir -p "$OUT_DIR"

KEY_FILE="$OUT_DIR/$SERVER_NAME.key"
CSR_FILE="$OUT_DIR/$SERVER_NAME.csr"
CRT_FILE="$OUT_DIR/$SERVER_NAME.crt"
FULLCHAIN_FILE="$OUT_DIR/$SERVER_NAME.fullchain.pem"
CONF_FILE="$OUT_DIR/$SERVER_NAME.cnf"

echo "==> Generating private key: $KEY_FILE"
openssl genrsa -out "$KEY_FILE" 2048

echo "==> Writing OpenSSL config: $CONF_FILE"
cat > "$CONF_FILE" <<EOF
[ req ]
default_bits       = 2048
prompt             = no
default_md         = sha256
req_extensions     = v3_req
distinguished_name = dn

[ dn ]
CN = $SERVER_NAME

[ v3_req ]
subjectAltName = @alt_names
extendedKeyUsage = serverAuth

[ alt_names ]
DNS.1 = $SERVER_NAME
DNS.2 = localhost
IP.1  = 127.0.0.1
IP.2  = ::1
EOF

echo "==> Generating CSR: $CSR_FILE"
openssl req -new \
  -key "$KEY_FILE" \
  -out "$CSR_FILE" \
  -config "$CONF_FILE"

echo "==> Signing certificate with CA: $CRT_FILE"
openssl x509 -req \
  -in "$CSR_FILE" \
  -CA "$CA_CERT" \
  -CAkey "$CA_KEY" \
  -CAcreateserial \
  -out "$CRT_FILE" \
  -days "$DAYS" \
  -sha256 \
  -extfile "$CONF_FILE" \
  -extensions v3_req

echo "==> Creating fullchain: $FULLCHAIN_FILE"
cat "$CRT_FILE" "$CA_CERT" > "$FULLCHAIN_FILE"

echo
echo "Done. Files generated in: $OUT_DIR"
ls -1 "$OUT_DIR"
echo
echo "Quick sanity check of SAN:"
openssl x509 -in "$FULLCHAIN_FILE" -text -noout | sed -n '/Subject:/p;/Subject Alternative Name:/,/X509v3/p'

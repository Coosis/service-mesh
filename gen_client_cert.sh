#!/usr/bin/env bash
set -euo pipefail

# === Config ===
# Override via env if needed:
#   CA_CERT=rootCA.crt CA_KEY=rootCA.key ./gen_client_cert.sh ...
CA_CERT="${CA_CERT:-rootCA.crt}"
CA_KEY="${CA_KEY:-rootCA.key}"
DAYS="${DAYS:-3650}"

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 <client_name> [out_dir]"
  echo "Example: $0 sidecar-b certs/clients/sidecar-b"
  exit 1
fi

CLIENT_NAME="$1"
OUT_DIR="${2:-certs/clients/$CLIENT_NAME}"

if [[ ! -f "$CA_CERT" || ! -f "$CA_KEY" ]]; then
  echo "ERROR: CA files not found."
  echo "  Expected:"
  echo "    CA_CERT=$CA_CERT"
  echo "    CA_KEY=$CA_KEY"
  exit 1
fi

mkdir -p "$OUT_DIR"

KEY_FILE="$OUT_DIR/$CLIENT_NAME.key"
CSR_FILE="$OUT_DIR/$CLIENT_NAME.csr"
CRT_FILE="$OUT_DIR/$CLIENT_NAME.crt"
FULLCHAIN_FILE="$OUT_DIR/$CLIENT_NAME.fullchain.pem"
CONF_FILE="$OUT_DIR/$CLIENT_NAME.cnf"

echo "==> Generating client private key: $KEY_FILE"
openssl genrsa -out "$KEY_FILE" 2048

echo "==> Writing OpenSSL client config: $CONF_FILE"
cat > "$CONF_FILE" <<EOF
[ req ]
default_bits       = 2048
prompt             = no
default_md         = sha256
req_extensions     = v3_req
distinguished_name = dn

[ dn ]
CN = $CLIENT_NAME

[ v3_req ]
extendedKeyUsage = clientAuth
subjectAltName   = @alt_names

[ alt_names ]
DNS.1 = $CLIENT_NAME
EOF

echo "==> Generating client CSR: $CSR_FILE"
openssl req -new \
  -key "$KEY_FILE" \
  -out "$CSR_FILE" \
  -config "$CONF_FILE"

echo "==> Signing client certificate with CA: $CRT_FILE"
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

echo "==> Creating client fullchain: $FULLCHAIN_FILE"
cat "$CRT_FILE" "$CA_CERT" > "$FULLCHAIN_FILE"

echo
echo "Done. Client certs generated in: $OUT_DIR"
ls -1 "$OUT_DIR"
echo
echo "Quick sanity check:"
openssl x509 -in "$FULLCHAIN_FILE" -text -noout | sed -n '/Subject:/p;/Subject Alternative Name:/,/X509v3/p;/Extended Key Usage:/p'

#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
ca_dir="${1:-../root-ca}"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
umask 077
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out "$tmp/key.pem" 2>/dev/null
openssl req -new -key "$tmp/key.pem" -subj '/O=Cangling/CN=local.cangling.cn' -out "$tmp/request.pem"
cat > "$tmp/extensions" <<'EOF'
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
subjectAltName=DNS:local.cangling.cn,IP:127.0.0.1
EOF
printf '\n请输入 Cangling Update Ca Root 私钥密码（不会保存密码）：\n'
openssl x509 -req -in "$tmp/request.pem" -CA "$ca_dir/update-ca.crt.pem" -CAkey "$ca_dir/update-ca.key.pem" -set_serial "0x$(openssl rand -hex 16)" -days 3652 -sha256 -extfile "$tmp/extensions" -out "$tmp/cert.pem"
openssl verify -CAfile "$ca_dir/root.crt.pem" -untrusted "$ca_dir/update-ca.crt.pem" "$tmp/cert.pem"
mkdir -p src/local-https
cat "$tmp/cert.pem" "$ca_dir/update-ca.crt.pem" > src/local-https/cert-chain.pem
cp "$tmp/key.pem" src/local-https/key.pem
openssl x509 -in "$tmp/cert.pem" -noout -subject -issuer -dates -ext subjectAltName
printf '\n签发完成。证书及网站私钥已写入 src/local-https，未复制 CA 私钥。\n'

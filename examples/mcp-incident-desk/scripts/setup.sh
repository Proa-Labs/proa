#!/bin/sh
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

set -eu
cd "$(dirname "$0")/.."
umask 077
mkdir -p .local data
chmod 700 .local data
if [ ! -f .local/ca.pem ]; then
  openssl req -x509 -newkey rsa:3072 -nodes -days 30 -keyout .local/ca.key -out .local/ca.pem -subj '/CN=Incident Desk Local Development CA' >/dev/null 2>&1
fi
if [ ! -f .local/tls.pem ]; then
  openssl req -newkey rsa:2048 -nodes -keyout .local/tls.key -out .local/tls.csr -subj '/CN=localhost' >/dev/null 2>&1
  cat > .local/tls.ext <<'CERT'
subjectAltName=DNS:localhost,IP:127.0.0.1
extendedKeyUsage=serverAuth
basicConstraints=CA:FALSE
CERT
  openssl x509 -req -in .local/tls.csr -CA .local/ca.pem -CAkey .local/ca.key -CAcreateserial -out .local/tls.pem -days 30 -extfile .local/tls.ext >/dev/null 2>&1
fi
if [ ! -f .env ]; then cp .env.example .env; fi
printf '%s\n' 'Local TLS ready. To trust this example CA in your login keychain, review and run:' 'security add-trusted-cert -r trustRoot -k "$HOME/Library/Keychains/login.keychain-db" .local/ca.pem' 'Then run bash scripts/dev.sh from this directory.'

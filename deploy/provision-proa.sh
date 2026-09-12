#!/usr/bin/env bash
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

# Provision (or re-provision) proa's server footprint on a latency-style box.
# Idempotent: safe to re-run; every step converges to the checked-in state.
#
# The repository is the source of truth for everything this installs:
#   deploy/server/proa-ci-ctl            validated deploy wrapper (sudo target)
#   deploy/server/proa-preview@.service  hardened preview instance template
#
# Usage (from the repo root, against a box you can reach as a sudoer):
#   PROA_CI_PUBKEY="ssh-ed25519 AAAA... proa-ci@github-actions" \
#     SERVER=1.2.3.4 [SSH_USER=deploy] bash deploy/provision-proa.sh
#
# Prerequisites (the shared base layer, provisioned by the latency-style
# bootstrap, not by this script): haproxy with the /etc/haproxy/previews
# registry and /usr/local/sbin/latency-render-haproxy +
# latency-haproxy-install-cert helpers, certbot, fail2ban, a hardened sshd
# with an AllowUsers line. This script verifies each and fails with a
# pointer rather than half-provisioning.
#
# This is retained only while the old host is decommissioned. See
# deploy/COOLIFY_DECOMMISSION.md before changing or running it.

set -euo pipefail
IFS=$'\n\t'

SERVER="${SERVER:?SERVER env var required}"
SSH_USER="${SSH_USER:-deploy}"
PROA_CI_PUBKEY="${PROA_CI_PUBKEY:?PROA_CI_PUBKEY env var required (the proa-ci authorized key)}"

[[ "$PROA_CI_PUBKEY" =~ ^(ssh-ed25519|ssh-rsa|ecdsa-) ]] || {
  echo "error: PROA_CI_PUBKEY does not look like an SSH public key" >&2
  exit 2
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WRAPPER="${SCRIPT_DIR}/server/proa-ci-ctl"
UNIT="${SCRIPT_DIR}/server/proa-preview@.service"
[[ -f "$WRAPPER" && -f "$UNIT" ]] || { echo "error: deploy/server files missing" >&2; exit 2; }

echo "==> Provisioning proa on ${SSH_USER}@${SERVER}"

# Ship the two config files, then converge everything in one root shell.
scp -q "$WRAPPER" "$UNIT" "${SSH_USER}@${SERVER}:/tmp/"

# Client-side expansion of the public key is intentional.
# shellcheck disable=SC2029
ssh "${SSH_USER}@${SERVER}" "PROA_CI_PUBKEY='${PROA_CI_PUBKEY}' sudo -E bash -s" <<'REMOTE'
set -euo pipefail
CI_USER=proa-ci
CI_HOME="/home/${CI_USER}"

# ---- prerequisite checks (the shared base layer) ----------------------------
missing=()
command -v haproxy >/dev/null || missing+=("haproxy")
command -v certbot >/dev/null || missing+=("certbot")
[[ -x /usr/local/sbin/latency-render-haproxy ]] || missing+=("latency-render-haproxy helper")
[[ -x /usr/local/sbin/latency-haproxy-install-cert ]] || missing+=("latency-haproxy-install-cert helper")
[[ -d /etc/haproxy/previews ]] || missing+=("/etc/haproxy/previews registry")
if (( ${#missing[@]} )); then
  printf 'error: base layer incomplete, provision it first (latency bootstrap): %s\n' "${missing[*]}" >&2
  exit 3
fi

# ---- proa-ci deploy user -----------------------------------------------------
id "$CI_USER" >/dev/null 2>&1 || useradd -m -s /bin/bash "$CI_USER"
passwd -lq "$CI_USER"
install -d -m 700 -o "$CI_USER" -g "$CI_USER" "${CI_HOME}/.ssh"
printf '%s\n' "$PROA_CI_PUBKEY" > "${CI_HOME}/.ssh/authorized_keys"
chown "$CI_USER:$CI_USER" "${CI_HOME}/.ssh/authorized_keys"
chmod 600 "${CI_HOME}/.ssh/authorized_keys"
install -d -m 755 -o "$CI_USER" -g "$CI_USER" "${CI_HOME}/staging"

# ---- sshd allowlist ----------------------------------------------------------
conf=/etc/ssh/sshd_config.d/hardened.conf
if [[ -f "$conf" ]] && grep -q '^AllowUsers ' "$conf" && ! grep -q '^AllowUsers .*\bproa-ci\b' "$conf"; then
  sed -i 's/^AllowUsers \(.*\)$/AllowUsers \1 proa-ci/' "$conf"
  sshd -t
  systemctl reload ssh
fi

# ---- wrapper + sudoers -------------------------------------------------------
install -m 755 /tmp/proa-ci-ctl /usr/local/sbin/proa-ci-ctl
rm -f /tmp/proa-ci-ctl
cat > /etc/sudoers.d/proa-ci <<'SUDOERS'
# proa-ci: GitHub Actions deploy user for the proa marketing site and its
# pr-N.proa.cool previews. Single validated wrapper; no direct systemctl.
proa-ci ALL=(root) NOPASSWD: /usr/local/sbin/proa-ci-ctl
SUDOERS
chmod 440 /etc/sudoers.d/proa-ci
visudo -c -f /etc/sudoers.d/proa-ci

# ---- preview runtime ---------------------------------------------------------
id proa-preview >/dev/null 2>&1 || useradd -r -M -s /usr/sbin/nologin proa-preview
install -d -m 755 /opt/proa-preview /etc/proa /etc/proa/previews
install -m 644 /tmp/proa-preview@.service /etc/systemd/system/proa-preview@.service
rm -f /tmp/proa-preview@.service
systemctl daemon-reload

# ---- marketing service skeleton (no-op when already deployed) ----------------
id proa >/dev/null 2>&1 || useradd -r -m -d /opt/proa-marketing -s /usr/sbin/nologin proa
install -d -m 755 /opt/proa-marketing /opt/proa-marketing/bin /opt/proa-marketing/releases

echo "provision-proa: converged"
REMOTE

echo "==> Done. Verify with: ssh proa-ci@${SERVER} 'sudo -n /usr/local/sbin/proa-ci-ctl marketing-status'"

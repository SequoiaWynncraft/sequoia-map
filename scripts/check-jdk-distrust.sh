#!/bin/sh
# Fails when the JDK in JAVA_HOME lacks the vendor distrust entries; see config/security/README.md.
set -eu
while IFS= read -r fingerprint; do
  case "$fingerprint" in Algorithm=*|\#*|'') continue ;; esac
  grep -qxF "$fingerprint" "$JAVA_HOME/lib/security/blocked.certs" || {
    printf 'JDK certificate distrust data is outdated; see config/security/README.md\n' >&2
    exit 1
  }
done < "$(dirname "$0")/../config/security/blocked.certs"

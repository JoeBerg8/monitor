#!/bin/zsh
set -euo pipefail

readonly IDENTITY_NAME="monitor Local Code Signing"
readonly SCRIPT_DIRECTORY="${0:A:h}"
readonly LOGIN_KEYCHAIN="${HOME}/Library/Keychains/login.keychain-db"
readonly IMPORT_PASSWORD="$(openssl rand -hex 32)"

if security find-identity -v -p codesigning "${LOGIN_KEYCHAIN}" \
  | grep -Fq "\"${IDENTITY_NAME}\""; then
  print "${IDENTITY_NAME} already exists."
  exit 0
fi

SIGNING_TEMP_DIRECTORY="$(mktemp -d /private/tmp/monitor-signing.XXXXXX)"
cleanup() {
  if [[ "${SIGNING_TEMP_DIRECTORY}" == /private/tmp/monitor-signing.* ]]; then
    rm -rf -- "${SIGNING_TEMP_DIRECTORY}"
  fi
}
trap cleanup EXIT

openssl req \
  -new \
  -x509 \
  -newkey rsa:3072 \
  -sha256 \
  -nodes \
  -days 3650 \
  -config "${SCRIPT_DIRECTORY}/monitor-local-root-ca.cnf" \
  -keyout "${SIGNING_TEMP_DIRECTORY}/root-private-key.pem" \
  -out "${SIGNING_TEMP_DIRECTORY}/root-certificate.pem"

openssl req \
  -new \
  -newkey rsa:3072 \
  -sha256 \
  -nodes \
  -config "${SCRIPT_DIRECTORY}/monitor-local-codesign.cnf" \
  -keyout "${SIGNING_TEMP_DIRECTORY}/private-key.pem" \
  -out "${SIGNING_TEMP_DIRECTORY}/certificate-request.pem"

openssl x509 \
  -req \
  -sha256 \
  -days 3650 \
  -in "${SIGNING_TEMP_DIRECTORY}/certificate-request.pem" \
  -CA "${SIGNING_TEMP_DIRECTORY}/root-certificate.pem" \
  -CAkey "${SIGNING_TEMP_DIRECTORY}/root-private-key.pem" \
  -CAcreateserial \
  -extfile "${SCRIPT_DIRECTORY}/monitor-local-codesign.cnf" \
  -extensions extensions \
  -out "${SIGNING_TEMP_DIRECTORY}/certificate.pem"

openssl pkcs12 \
  -export \
  -legacy \
  -name "${IDENTITY_NAME}" \
  -inkey "${SIGNING_TEMP_DIRECTORY}/private-key.pem" \
  -in "${SIGNING_TEMP_DIRECTORY}/certificate.pem" \
  -certfile "${SIGNING_TEMP_DIRECTORY}/root-certificate.pem" \
  -out "${SIGNING_TEMP_DIRECTORY}/identity.p12" \
  -passout "pass:${IMPORT_PASSWORD}"

security add-trusted-cert \
  -r trustRoot \
  -k "${LOGIN_KEYCHAIN}" \
  "${SIGNING_TEMP_DIRECTORY}/root-certificate.pem"

security import "${SIGNING_TEMP_DIRECTORY}/identity.p12" \
  -k "${LOGIN_KEYCHAIN}" \
  -P "${IMPORT_PASSWORD}" \
  -x \
  -T /usr/bin/codesign \
  -T /usr/bin/security

security find-identity -v -p codesigning "${LOGIN_KEYCHAIN}" \
  | grep -F "\"${IDENTITY_NAME}\""

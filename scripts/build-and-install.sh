#!/bin/zsh
set -euo pipefail

readonly IDENTITY_NAME="monitor Local Code Signing"
readonly REPOSITORY_DIRECTORY="${0:A:h:h}"
readonly DERIVED_DATA_DIRECTORY="${REPOSITORY_DIRECTORY}/build/DerivedData"
readonly BUILT_APP="${DERIVED_DATA_DIRECTORY}/Build/Products/Debug/monitor.app"
readonly INSTALL_DIRECTORY="/Applications"
readonly INSTALLED_APP="${INSTALL_DIRECTORY}/monitor.app"
readonly LOGIN_KEYCHAIN="${HOME}/Library/Keychains/login.keychain-db"

if ! security find-identity -v -p codesigning "${LOGIN_KEYCHAIN}" \
  | grep -Fq "\"${IDENTITY_NAME}\""; then
  print -u2 "Missing ${IDENTITY_NAME}. Run scripts/setup-local-signing.sh first."
  exit 1
fi

cd "${REPOSITORY_DIRECTORY}"
xcodegen generate
xcodebuild \
  -project monitor.xcodeproj \
  -scheme monitor \
  -configuration Debug \
  -derivedDataPath "${DERIVED_DATA_DIRECTORY}" \
  build

codesign --force --sign "${IDENTITY_NAME}" --timestamp=none \
  "${BUILT_APP}/Contents/MacOS/monitor.debug.dylib"
codesign --force --sign "${IDENTITY_NAME}" --timestamp=none \
  "${BUILT_APP}/Contents/MacOS/__preview.dylib"
codesign --force --sign "${IDENTITY_NAME}" --timestamp=none \
  --identifier com.joeberg.monitor \
  "${BUILT_APP}"

mkdir -p "${INSTALL_DIRECTORY}"
ditto "${BUILT_APP}" "${INSTALLED_APP}"
codesign --display --requirements - "${INSTALLED_APP}"

print "Installed ${INSTALLED_APP}"
print "Quit any running monitor instance, then run:"
print "  open '${INSTALLED_APP}'"

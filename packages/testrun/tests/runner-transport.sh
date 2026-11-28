#!/bin/bash
# Exercise production runner authentication/admission over real loopback sockets.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
build="$(mktemp -d "${TMPDIR:-/tmp}/testrun-runner-transport.XXXXXX")"
trap 'rm -rf "$build"' EXIT
frameworks="$(xcrun --show-sdk-platform-path)/Developer/Library/Frameworks"
xcrun swiftc -swift-version 5 -warnings-as-errors -parse-as-library \
  -F "$frameworks" -Xlinker -rpath -Xlinker "$frameworks" \
  "$root/runner/ios/RunnerRequests.swift" "$root/runner/ios/RunnerTransport.swift" \
  "$root/tests/fixtures/runner-transport/server.swift" -o "$build/runner-fixture"
node "$root/tests/fixtures/runner-transport/client.mjs" "$build/runner-fixture"

#!/bin/bash
# Compile the production adapter against a host-only XCTest boundary fixture.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
build="$(mktemp -d "${TMPDIR:-/tmp}/testrun-concrete-test.XXXXXX")"
trap 'rm -rf "$build"' EXIT
xcrun clang -fobjc-arc -fblocks -Wall -Wextra -Wno-unused-parameter \
  -I "$root/tests/fixtures/concrete-tap" -I "$root/runner/ios" \
  -framework Foundation -framework CoreGraphics \
  "$root/runner/ios/ConcreteTap.m" "$root/runner/ios/RunnerIssueCase.m" \
  "$root/runner/ios/FocusedInput.m" \
  "$root/runner/ios/RunnerQuiescence.m" \
  "$root/tests/fixtures/concrete-tap/main.m" \
  -o "$build/concrete-tap-tests"
"$build/concrete-tap-tests"

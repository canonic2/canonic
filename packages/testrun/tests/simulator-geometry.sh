#!/bin/bash
# Exercise the production ARC bridge with an MRC autorelease ownership fixture.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
build="$(mktemp -d "${TMPDIR:-/tmp}/testrun-geometry-test.XXXXXX")"
trap 'rm -rf "$build"' EXIT
xcrun clang -fobjc-arc -fblocks -c "$root/native/simulator.m" -o "$build/simulator.o"
xcrun clang -fno-objc-arc -fblocks -Wall -Wextra -Werror \
  "$root/tests/fixtures/simulator-geometry/main.m" "$build/simulator.o" \
  -framework Foundation -framework AppKit -framework CoreImage -framework ImageIO \
  -framework CoreGraphics -framework IOSurface -framework VideoToolbox \
  -framework CoreMedia -framework CoreVideo -framework AVFoundation \
  -o "$build/simulator-geometry-tests"
"$build/simulator-geometry-tests"

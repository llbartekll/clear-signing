#!/usr/bin/env bash
# Re-apply the 16 KB common-page-size linker flag to ubrn's generated
# bindings/react-native/android/CMakeLists.txt.
#
# `ubrn build android` regenerates CMakeLists.txt with only
# `-Wl,-z,max-page-size=16384`. Android 15+ page-size guidance for NDK r27
# and lower calls for both max-page-size and common-page-size. Keep these as
# separate linker flags so the generated line stays easy to audit.
#
# Idempotent: after substitution, the exact generated line no longer matches.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CMAKELISTS="$REPO_ROOT/bindings/react-native/android/CMakeLists.txt"

[ -f "$CMAKELISTS" ] || { echo "ERROR: $CMAKELISTS not found — run scripts/build-rn-android.sh first" >&2; exit 1; }

perl -0pi -e \
  'my $d = chr(36); my $replacement = qq{set(CMAKE_SHARED_LINKER_FLAGS "${d}{CMAKE_SHARED_LINKER_FLAGS} -Wl,-z,max-page-size=16384 -Wl,-z,common-page-size=16384")}; s|set\(CMAKE_SHARED_LINKER_FLAGS "[ \t]*(?:\$\{CMAKE_SHARED_LINKER_FLAGS\}[ \t]+)?-Wl,-z,max-page-size=16384(?:,-z,common-page-size=16384)?(?:[ \t]+-Wl,-z,common-page-size=16384)?"\)|$replacement|ge' \
  "$CMAKELISTS"

EXPECTED='set(CMAKE_SHARED_LINKER_FLAGS "${CMAKE_SHARED_LINKER_FLAGS} -Wl,-z,max-page-size=16384 -Wl,-z,common-page-size=16384")'
if grep -Fxq "$EXPECTED" "$CMAKELISTS"; then
  echo "wrote $CMAKELISTS (16 KB page-size flags applied)"
else
  echo "ERROR: failed to apply 16 KB common-page-size patch to $CMAKELISTS" >&2
  exit 1
fi

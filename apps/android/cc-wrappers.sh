#!/bin/sh
# Creates one compiler driver per Android target: the NDK's plain clang plus the target triple
# and API level. Works with every NDK layout, whether or not it ships versioned
# <triple><api>-clang wrapper scripts.
#
# Usage: cc-wrappers.sh <ndk-bin-dir> <out-dir> <api> <triple>...
#
# The target is appended after the caller's arguments: build tools pass an unversioned
# --target=<triple> of their own, and NDK r30 and later refuse unversioned triples.
NDK_BIN=$1
OUT=$2
API=$3
shift 3
for triple in "$@"; do
    printf '#!/bin/sh\nexec "%s/clang" "$@" --target=%s%s\n' "$NDK_BIN" "$triple" "$API" > "$OUT/cc-$triple"
    chmod +x "$OUT/cc-$triple"
done

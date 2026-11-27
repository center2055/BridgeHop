#!/bin/sh
# Creates one compiler driver per Android target: the plain clang of <bin-dir> plus the target
# triple and API level. Works with every NDK layout and with a device's own compiler (Termux),
# whether or not they ship versioned <triple><api>-clang wrapper scripts.
#
# Usage: cc-wrappers.sh <clang-bin-dir> <out-dir> <api> <interpreter> <triple>...
#
# The target is appended after the caller's arguments: build tools pass an unversioned
# --target=<triple> of their own, and NDK r30 and later refuse unversioned triples.
# <interpreter> is the absolute path of the shell that runs the wrappers: Android has no /bin/sh.
# HOPFORGE_CC_FLAGS (optional): extra driver flags written into every wrapper, for a flag that
# only the compiler in use understands (Termux's: -fno-termux-rpath keeps it from sending every
# program to the libraries of its own installation directory at run time).
NDK_BIN=$1
OUT=$2
API=$3
INTERP=$4
EXTRA="${HOPFORGE_CC_FLAGS:-}"
shift 4
for triple in "$@"; do
    printf '#!%s\nexec "%s/clang" "$@" --target=%s%s %s\n' "$INTERP" "$NDK_BIN" "$triple" "$API" "$EXTRA" > "$OUT/cc-$triple"
    chmod +x "$OUT/cc-$triple"
done

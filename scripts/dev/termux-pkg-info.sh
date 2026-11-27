#!/usr/bin/env bash
# Looks packages up in a Termux `Packages` index. Development helper: the facts about Termux
# (versions, dependencies, file names, checksums) must come from the repository itself.
#
#   scripts/dev/termux-pkg-info.sh <Packages-file> <package-name>...
set -euo pipefail

index=$1
shift
[ -f "$index" ] || { echo "usage: $0 <Packages-file> <package-name>..." >&2; exit 2; }

for name in "$@"; do
    awk -v want="$name" '
        function flush() {
            if (pkg == want) {
                printf "%s %s (%s)\n", pkg, version, arch
                if (depends != "") printf "  depends: %s\n", depends
                printf "  file: %s\n  size: %s bytes (%.1f MiB)\n", filename, size, size / 1048576
                printf "  installed: %s bytes (%.1f MiB)\n", installed, installed / 1048576
                printf "  sha256: %s\n", sha256
                found = 1
            }
            pkg = version = arch = depends = filename = size = installed = sha256 = ""
        }
        /^$/ { flush(); next }
        /^Package: / { pkg = substr($0, 10) }
        /^Version: / { version = substr($0, 10) }
        /^Architecture: / { arch = substr($0, 15) }
        /^Depends: / { depends = substr($0, 10) }
        /^Filename: / { filename = substr($0, 11) }
        /^Size: / { size = substr($0, 7) }
        /^Installed-Size: / { installed = substr($0, 17) * 1024 }
        /^SHA256: / { sha256 = substr($0, 9) }
        END { flush() }
        ' "$index" || true
done

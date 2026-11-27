#!/usr/bin/env bash
# Downloads Termux packages listed in a `Packages` index, checks each one against the SHA-256 the
# index gives, and unpacks it for inspection. Development helper: what is true about a Termux
# package must be read from the package itself, not remembered.
#
#   scripts/dev/termux-deb-fetch.sh <Packages-file> <output-dir> <package-name>...
# The index is expected to have been checked against the repository's signed `InRelease`.
set -euo pipefail

base='https://packages.termux.dev/apt/termux-main/'
index=$1
out=$2
shift 2

for name in "$@"; do
    # The stanza of one package: its file name and checksum.
    fields="$(awk -v want="$name" '
        /^$/ { if (pkg == want && filename != "") { print filename, sha256; exit } pkg = ""; }
        /^Package: / { pkg = substr($0, 10) }
        /^Filename: / { filename = substr($0, 11) }
        /^SHA256: / { sha256 = substr($0, 9) }
        ' "$index")"
    if [ -z "$fields" ]; then
        echo "$name: not in the index" >&2
        exit 1
    fi
    read -r file sha256 <<<"$fields"
    mkdir -p "$out"
    deb="$out/$(basename "$file")"
    if [ ! -f "$deb" ] || ! echo "$sha256  $deb" | sha256sum -c - > /dev/null 2>&1; then
        curl -fsSL --retry 5 --connect-timeout 20 -o "$deb.part" "$base$file"
        echo "$sha256  $deb.part" | sha256sum -c - > /dev/null
        mv "$deb.part" "$deb"
    fi
    dir="$out/$name"
    rm -rf "$dir"
    mkdir -p "$dir"
    (cd "$dir" && ar x "$deb" && tar -xf data.tar.xz)
    echo "$name: $(basename "$file") sha256 ok, $(find "$dir" -type f | wc -l) files"
done

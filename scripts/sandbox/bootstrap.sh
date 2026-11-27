#!/usr/bin/env bash
# Builds the toolchain that builds HopForge's Android app on a Debian x86_64 host without root.
# Nothing is installed system-wide: everything lands under the directory given as the only
# argument. Each download is checked against the checksum pinned below; for Google's SDK files
# the pin is additionally compared with Google's live manifest, so a changed file fails loudly
# instead of being used silently.
#
#   scripts/sandbox/bootstrap.sh <toolchain-dir>
#   . scripts/sandbox/env.sh <toolchain-dir>       # puts all of it on PATH
#
# What is installed, and where:
#   go/                            Go, for the pluggable-transport sidecars (this is GOROOT)
#   rust/  cargo/                  rustup: stable toolchain and the aarch64-linux-android target
#   android/build-tools/36.0.0/    aapt2, apksigner, zipalign
#   d8/                            D8 9.2 (the Termux `d8` package), see below
#   android/platforms/android-36/  android.jar
#   android/ndk/r30/               NDK r30, LLVM toolchain for linux-x86_64, arm64 sysroot only
#   kotlin/                        the Kotlin compiler (the Termux `kotlin` package: the upstream
#                                  distribution, architecture "all", checksum from the signed index)
# tor for the host (end-to-end test only) is not installed here: scripts/sandbox/host-tor.sh
set -euo pipefail

[ "$#" -eq 1 ] || { echo "usage: $0 <toolchain-dir>" >&2; exit 2; }
TC=$(mkdir -p "$1" && cd "$1" && pwd)
DL="$TC/dl"
mkdir -p "$DL" "$TC/logs"

log() { printf '[bootstrap] %s\n' "$*"; }
die() { printf '[bootstrap] %s\n' "$*" >&2; exit 1; }

# ---- pinned downloads -------------------------------------------------------------------
# Where each checksum comes from is named next to it. Google's manifest lists one <archive>
# per host OS; the pins below are the *linux* rows.
GO_VERSION=1.27.4
GO_TARBALL="go${GO_VERSION}.linux-amd64.tar.gz"
GO_SIZE=70680823
GO_SHA256=7226736c2339719cda28fbf3a54d71aa63bcefe376e98cef00034268dab24840       # dl.google.com/go/<tarball>.sha256
RUSTUP_SHA256=b1c8cb14e30889e7e2999e6b69e62d9e9b9ffc0f4df8180564d306d9680e6c55    # static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init.sha256
MANIFEST=https://dl.google.com/android/repository/repository2-3.xml
BT_NAME=build-tools_r36_linux.zip
BT_SIZE=63737259
BT_SHA1=b0b6376977657e8ad9b969bacf4093601da2c6fb                                       # the manifest
PLATFORM_NAME=platform-36_r02.zip
PLATFORM_SIZE=65878410
PLATFORM_SHA1=2c1a80dd4d9f7d0e6dd336ec603d9b5c55a6f576                                 # the manifest
NDK_NAME=android-ndk-r30-linux.zip
NDK_SIZE=738633529
NDK_SHA1=5107f898313790e449e87eee2183d9a20602dee9                                       # the manifest
NDK_BUILD=16248370                                                                      # ndk;30.0.<build>
TERMUX=https://packages.termux.dev/apt/termux-main
KOTLIN_FILE=pool/main/k/kotlin/kotlin_2.4.21_all.deb
KOTLIN_SHA256=cf62260a082379c97d91032d9f4f1b08c0c9de1290f20dc84f5a3e45f056c6b7   # the Termux index, signed by the Termux maintainers

# ---- helpers ----------------------------------------------------------------------------
# check <file> <sha256|sha1> <digest> [size]: succeeds when the file has exactly that content.
check() {
    local file=$1 algo=$2 want=$3 size=${4:-}
    [ -f "$file" ] || return 1
    if [ -n "$size" ] && [ "$(stat -c %s "$file")" != "$size" ]; then
        log "$file: size $(stat -c %s "$file"), expected $size"
        return 1
    fi
    echo "$want  $file" | "${algo}sum" -c - > /dev/null 2>&1
}

# download <url> <algo> <digest> [size]: leaves the verified file in $FILE.
download() {
    local url=$1 algo=$2 want=$3 size=${4:-}
    FILE="$DL/$(basename "$url")"
    if [ -f "$FILE" ] && check "$FILE" "$algo" "$want" "$size"; then
        log "$(basename "$FILE"): already downloaded, checksum ok"
        return
    fi
    log "downloading $url"
    curl -fL --retry 5 --retry-all-errors --connect-timeout 30 -o "$FILE.part" "$url"
    check "$FILE.part" "$algo" "$want" "$size" || die "$(basename "$FILE"): the download does not match the pinned $algo/size"
    mv "$FILE.part" "$FILE"
}

# manifest_row <file-name>: prints "<size> <sha1>" of the archive with that url whose host is
# linux, or that has no host restriction. The rows are: size, checksum, url, [host-os].
manifest_row() {
    awk -v want="$1" '
        /<archive>/ { size = ""; sha = ""; url = ""; host = "" }
        /<size>/ { s = $0; gsub(/<\/?size>/, "", s); gsub(/^[ \t]+/, "", s); size = s }
        /<checksum type="sha1">/ { s = $0; sub(/.*<checksum type="sha1">/, "", s); sub(/<\/checksum>.*/, "", s); sha = s }
        /<url>/ { s = $0; sub(/.*<url>/, "", s); sub(/<\/url>.*/, "", s); url = s }
        /<host-os>/ { s = $0; sub(/.*<host-os>/, "", s); sub(/<\/host-os>.*/, "", s); host = s }
        /<\/archive>/ && url == want && (host == "" || host == "linux") { print size, sha; found = 1 }
        END { exit found ? 0 : 1 }
        ' "$DL/$(basename "$MANIFEST")"
}

# google <name> <size> <sha1>: checks the pin against Google's manifest, then downloads.
google() {
    local name=$1 size=$2 sha1=$3 row
    read -r msize msha1 <<<"$(manifest_row "$name")" || die "$name: not listed in Google's manifest"
    [ "$msize $msha1" = "$size $sha1" ] \
        || die "$name: pinned size/sha1 ($size $sha1) differ from Google's manifest ($msize $msha1)"
    download "https://dl.google.com/android/repository/$name" sha1 "$sha1" "$size"
}

# The single top-level directory of an extracted zip (build-tools zips do not carry their
# version in that name, so it is located rather than assumed).
top_dir() { # top_dir <dir>
    local dir
    dir=$(find "$1" -mindepth 1 -maxdepth 1 -type d)
    [ "$(printf '%s\n' "$dir" | wc -l)" -eq 1 ] || die "$1: expected one top-level directory"
    printf '%s\n' "$dir"
}

# ---- Go ---------------------------------------------------------------------------------
if [ ! -x "$TC/go/bin/go" ]; then
    download "https://dl.google.com/go/$GO_TARBALL" sha256 "$GO_SHA256" "$GO_SIZE"
    rm -rf "$TC/go.tmp" "$TC/go" && mkdir -p "$TC/go.tmp"
    tar -xzf "$FILE" -C "$TC/go.tmp"
    mv "$TC/go.tmp/go" "$TC/go" && rmdir "$TC/go.tmp"
fi
log "go: $("$TC/go/bin/go" version)"

# ---- Rust -------------------------------------------------------------------------------
# rustup-init installs the stable toolchain; the Android target is added by rustup itself
# (`rustup target add` has no --component option).
if [ ! -x "$TC/cargo/bin/cargo" ]; then
    download "https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init" sha256 "$RUSTUP_SHA256"
    chmod +x "$FILE"
    RUSTUP_HOME="$TC/rust" CARGO_HOME="$TC/cargo" "$FILE" -y --no-modify-path --profile minimal \
        --default-toolchain stable --component rustfmt --component clippy \
        > "$TC/logs/rustup-init.log" 2>&1 \
        || { cat "$TC/logs/rustup-init.log"; die "rustup-init failed"; }
fi
RUSTUP_HOME="$TC/rust" CARGO_HOME="$TC/cargo" "$TC/cargo/bin/rustup" target add aarch64-linux-android > /dev/null
log "rust: $(RUSTUP_HOME="$TC/rust" CARGO_HOME="$TC/cargo" "$TC/cargo/bin/rustc" --version), targets: $(RUSTUP_HOME="$TC/rust" CARGO_HOME="$TC/cargo" "$TC/cargo/bin/rustup" target list --installed | tr '\n' ' ')"

# ---- Android SDK pieces ----------------------------------------------------------------
curl -fsSL --retry 5 --connect-timeout 30 -o "$DL/$(basename "$MANIFEST")" "$MANIFEST"

if [ ! -x "$TC/android/build-tools/36.0.0/aapt2" ]; then
    google "$BT_NAME" "$BT_SIZE" "$BT_SHA1"
    rm -rf "$TC/android/.bt.tmp" && mkdir -p "$TC/android/.bt.tmp"
    unzip -q "$FILE" -d "$TC/android/.bt.tmp"
    mkdir -p "$TC/android/build-tools"
    rm -rf "$TC/android/build-tools/36.0.0"
    mv "$(top_dir "$TC/android/.bt.tmp")" "$TC/android/build-tools/36.0.0"
    rmdir "$TC/android/.bt.tmp"
fi

if [ ! -f "$TC/android/platforms/android-36/android.jar" ]; then
    google "$PLATFORM_NAME" "$PLATFORM_SIZE" "$PLATFORM_SHA1"
    rm -rf "$TC/android/.platform.tmp" && mkdir -p "$TC/android/.platform.tmp"
    unzip -q "$FILE" -d "$TC/android/.platform.tmp"
    src="$(top_dir "$TC/android/.platform.tmp")"
    [ -f "$src/android.jar" ] || die "platform zip has no android.jar"
    grep -q '^AndroidVersion.ApiLevel=36$' "$src/source.properties" || die "platform zip is not API 36"
    mkdir -p "$TC/android/platforms"
    rm -rf "$TC/android/platforms/android-36"
    mv "$src" "$TC/android/platforms/android-36"
    rmdir "$TC/android/.platform.tmp"
fi

# The NDK zip (739 MB compressed) holds a 758 MB bin/ directory of which the build uses nine
# programs, a compiler runtime for eight targets, and a sysroot per ABI. Only the LLVM
# toolchain's programs that are needed, the arm64 compiler runtime and the arm64 sysroot are
# extracted: the rest is not used and does not fit next to the builds. libunwind.a is part of
# that: the NDK r30 driver asks the linker for `-l:libunwind.a` on every link, and the file lives
# in the compiler's own lib/linux/aarch64 directory (the sysroot has none any more).
# SKIP_NDK=1 leaves it out (the host-side end-to-end test does not need it).
NDK="$TC/android/ndk/r30"
if [ "${SKIP_NDK:-0}" != 1 ] && [ ! -x "$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/clang" ]; then
    google "$NDK_NAME" "$NDK_SIZE" "$NDK_SHA1"
    root=android-ndk-r30
    tc=$root/toolchains/llvm/prebuilt/linux-x86_64
    rm -rf "$TC/android/.ndk.tmp" && mkdir -p "$TC/android/.ndk.tmp"
    unzip -q "$FILE" "$root/source.properties" "$root/NOTICE" "$root/NOTICE.toolchain" \
        "$tc/bin/*" "$tc/lib/clang/*/include/*" "$tc/lib/clang/*/share/*" \
        "$tc/lib/clang/*/lib/linux/libclang_rt.builtins-aarch64-android.a" \
        "$tc/lib/clang/*/lib/linux/aarch64/libunwind.a" \
        "$tc/sysroot/NOTICE" "$tc/sysroot/usr/include/*" "$tc/sysroot/usr/lib/aarch64-linux-android/*" \
        -d "$TC/android/.ndk.tmp"
    grep -q "^Pkg.Revision = 30.0.$NDK_BUILD" "$TC/android/.ndk.tmp/$root/source.properties" \
        || die "NDK is not build $NDK_BUILD"
    # Everything in bin/ that is not a driver, a wrapper or a small tool of the LLVM set.
    find "$TC/android/.ndk.tmp/$tc/bin" -maxdepth 1 -type f -size +2M \
        ! -name 'clang-21' ! -name 'lld' ! -name 'llvm-ar' ! -name 'llvm-nm' ! -name 'llvm-objcopy' \
        ! -name 'llvm-objdump' ! -name 'llvm-readobj' ! -name 'llvm-size' ! -name 'llvm-symbolizer' \
        -delete
    mkdir -p "$TC/android/ndk"
    rm -rf "$NDK"
    mv "$TC/android/.ndk.tmp/$root" "$NDK"
    rm -rf "$TC/android/.ndk.tmp"
fi
if [ -x "$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/clang" ]; then
    log "ndk: $(grep '^Pkg.Revision' "$NDK/source.properties"), $("$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/clang" --version | head -n 1)"
else
    log "ndk: not installed (SKIP_NDK)"
fi

# ---- Kotlin -----------------------------------------------------------------------------
if [ ! -f "$TC/kotlin/lib/kotlin-compiler.jar" ]; then
    download "$TERMUX/$KOTLIN_FILE" sha256 "$KOTLIN_SHA256"
    rm -rf "$TC/.kotlin.tmp" && mkdir -p "$TC/.kotlin.tmp"
    (cd "$TC/.kotlin.tmp" && ar x "$FILE" data.tar.xz && tar -xf data.tar.xz)
    rm -rf "$TC/kotlin"
    mv "$TC/.kotlin.tmp/data/data/com.termux/files/usr/opt/kotlin" "$TC/kotlin"
    rm -rf "$TC/.kotlin.tmp"
    # The package points its launchers at Termux's shell; here the host shell runs them.
    sed -i '1s|^#!/data/data/com.termux/files/usr/bin/env bash|#!/usr/bin/env bash|' "$TC"/kotlin/bin/kotlin "$TC"/kotlin/bin/kotlinc
fi
log "kotlin: $("$TC/kotlin/bin/kotlinc" -version 2>&1)"

# ---- D8 ---------------------------------------------------------------------------------
# The dexer. build-tools 36 brings D8 8.10.9, which cannot read the metadata of classes compiled
# by Kotlin 2.4 and prints a warning for every class; Termux's `d8` package is the newer D8 (9.2),
# a jar that runs on any host. The builds in Termux use the same one ($PREFIX/bin/d8).
D8_FILE=pool/main/d/d8/d8_37.0.0_all.deb
D8_SHA256=3d29a2ed2ae86ba0f06a39071b5c4b848370019f3aa24bad34ea293938ff5335   # the Termux index
if [ ! -f "$TC/d8/d8.jar" ]; then
    download "$TERMUX/$D8_FILE" sha256 "$D8_SHA256"
    rm -rf "$TC/.d8.tmp" && mkdir -p "$TC/.d8.tmp"
    (cd "$TC/.d8.tmp" && ar x "$FILE" data.tar.xz && tar -xf data.tar.xz)
    mkdir -p "$TC/d8"
    rm -f "$TC/d8/d8.jar"
    mv "$TC/.d8.tmp/data/data/com.termux/files/usr/share/java/d8.jar" "$TC/d8/d8.jar"
    rm -rf "$TC/.d8.tmp"
fi
if [ ! -x "$TC/d8/d8" ]; then
    cat > "$TC/d8/d8" <<EOF
#!/usr/bin/env bash
# What Termux's \$PREFIX/bin/d8 does, with the copy of d8.jar that lives next to it.
exec java -cp "$TC/d8/d8.jar" com.android.tools.r8.D8 "\$@"
EOF
    chmod +x "$TC/d8/d8"
fi
command -v java > /dev/null 2>&1 || die "D8 needs a JDK on PATH (java), as does the signing step (keytool)"
log "d8: $("$TC/d8/d8" --version)"

log "toolchain ready in $TC"

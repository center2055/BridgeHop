#!/usr/bin/env bash
# Builds the pluggable-transport (PT) client sidecars from the sources pinned in go.mod/go.sum.
#
# Usage:  pt-sidecars/build.sh <output-dir> [target ...]
# Targets (default: all):  host  android/arm64-v8a  android/armeabi-v7a  android/x86_64
#
# Android targets are plain executables named lib*.so: the Android packaging format only ships
# files called lib<name>.so in jniLibs/, and with extractNativeLibs="true" the app can execute
# them straight from its nativeLibraryDir.
#
# Android targets are built with cgo and need a C compiler for their ABI in ANDROID_CC_<abi>
# (hyphens become underscores: ANDROID_CC_arm64_v8a). Without cgo, Go resolves names with its
# own resolver, which on Android has no resolv.conf to read and ends up asking 127.0.0.1:53
# (go/src/net/conf_android.go: "DNS requests don't work on Android, so prefer the cgo
# resolver"). With cgo the binaries use Android's getaddrinfo and need libc.so, libdl.so and
# liblog.so, all of which are on every Android device.
#
# Every binary is a stock upstream client at the pinned version; this project adds no code to
# them. Each module's sources are verified against the public checksum database before use.
set -euo pipefail

cd "$(dirname "$0")"

if [ "$#" -eq 0 ]; then
    echo "usage: $0 <output-dir> [target ...]" >&2
    exit 2
fi
OUT=$1
shift
if [ "$#" -eq 0 ]; then
    set -- host android/arm64-v8a android/armeabi-v7a android/x86_64
fi
OUT=$(mkdir -p "$OUT" && cd "$OUT" && pwd)

LYREBIRD=gitlab.torproject.org/tpo/anti-censorship/pluggable-transports/lyrebird/cmd/lyrebird
SNOWFLAKE=gitlab.torproject.org/tpo/anti-censorship/pluggable-transports/snowflake/v2/client
WEBTUNNEL=gitlab.torproject.org/tpo/anti-censorship/pluggable-transports/webtunnel/main/client
CONJURE=gitlab.torproject.org/tpo/anti-censorship/pluggable-transports/conjure/client
DNSTT=www.bamsoftware.com/git/dnstt.git/dnstt-client

# Verify the pinned sources against the public checksum database before anything is compiled.
# -mod=readonly (the default) refuses any go.mod/go.sum drift.
for mod in . conjure; do
    (cd "$mod" && go mod verify | sed "s#^#[$mod] #")
done

# 16 KB pages: Android 15+ devices with a 16 KB page size need LOAD segments aligned to 16384.
# `-extldflags` goes to the C compiler that links a cgo build; the Go linker passes it on.
ldflags="-s -w -buildid= -checklinkname=0"

# The C compiler for an Android ABI, and stops the script when it is missing or unusable.
# Names with hyphens are not shell variables, so the ABI is mapped to ANDROID_CC_arm64_v8a etc.
android_cc() { # android_cc <abi>
    local abi=$1 var value
    var="ANDROID_CC_$(printf '%s' "$abi" | tr -- '-' '_')"
    value="${!var:-}"
    [ -n "$value" ] || { echo "android/$abi needs a C compiler: set $var to one for its target" >&2; exit 2; }
    [ -x "$value" ] || { echo "$var is not an executable file: $value" >&2; exit 2; }
    printf '%s\n' "$value"
}

# build_one <module-dir> <goos> <goarch> <abi-dir> <transport> <package>
build_one() {
    local module=$1 goos=$2 goarch=$3 abi=$4 transport=$5 pkg=$6 dir name cc
    if [ -n "$abi" ]; then
        dir=$OUT/$abi
        # An executable despite the name (see header comment).
        name="lib$transport.so"
        cc=$(android_cc "$abi")
        mkdir -p "$dir"
        (cd "$module" && GOOS=$goos GOARCH=$goarch CGO_ENABLED=1 CC="$cc" go build -trimpath -buildvcs=false \
            -ldflags "$ldflags -extldflags=-Wl,-z,max-page-size=16384" -o "$dir/$name" "$pkg")
    else
        dir=$OUT
        name=$transport
        [ "$goos" = windows ] && name="$transport.exe"
        mkdir -p "$dir"
        if [ "$goos" = android ]; then
            # Built on an Android device itself (Termux): its own tools target Android too.
            (cd "$module" && GOOS=$goos GOARCH=$goarch CGO_ENABLED=1 CC="${CC:-cc}" go build -trimpath -buildvcs=false \
                -ldflags "$ldflags -extldflags=-Wl,-z,max-page-size=16384" -o "$dir/$name" "$pkg")
        else
            (cd "$module" && GOOS=$goos GOARCH=$goarch CGO_ENABLED=0 go build -trimpath -buildvcs=false \
                -ldflags "$ldflags" -o "$dir/$name" "$pkg")
        fi
    fi
    # Report the exact module versions that went into the binary.
    go version -m "$dir/$name" | awk '$1 == "dep" || $1 == "mod" {print}' | sed 's#^#    #'
}

# build_set <goos> <goarch> <abi-dir-or-empty>
build_set() {
    # Fail before the first long build when a compiler is missing, not after it.
    [ -z "$3" ] || android_cc "$3" > /dev/null
    echo "[$1/$2] lyrebird"
    build_one . "$1" "$2" "$3" lyrebird "$LYREBIRD"
    echo "[$1/$2] snowflake"
    build_one . "$1" "$2" "$3" snowflake "$SNOWFLAKE"
    echo "[$1/$2] webtunnel"
    build_one . "$1" "$2" "$3" webtunnel "$WEBTUNNEL"
    echo "[$1/$2] conjure"
    build_one conjure "$1" "$2" "$3" conjure "$CONJURE"
    echo "[$1/$2] dnstt-client"
    build_one . "$1" "$2" "$3" dnstt-client "$DNSTT"
}

for target in "$@"; do
    case $target in
        host)                 build_set "$(go env GOOS)" "$(go env GOARCH)" "" ;;
        android/arm64-v8a)    build_set android arm64 arm64-v8a ;;
        android/armeabi-v7a)  build_set android arm armeabi-v7a ;;
        android/x86_64)       build_set android amd64 x86_64 ;;
        *) echo "unknown target: $target" >&2; exit 2 ;;
    esac
done

echo "sidecars built in $OUT"

#!/bin/sh
# Builds the pluggable-transport (PT) client sidecars from the sources pinned in go.mod/go.sum.
#
# Usage:  pt-sidecars/build.sh <output-dir> [target ...]
# Targets (default: all):  host  android/arm64-v8a  android/armeabi-v7a  android/x86_64
#
# Android targets are plain executables named lib*.so: the Android packaging format only ships
# files called lib<name>.so in jniLibs/, and with extractNativeLibs="true" the app can execute
# them straight from its nativeLibraryDir. CGO is disabled so the build needs no NDK.
#
# Every binary is a stock upstream client at the pinned version; this project adds no code to
# them. Each module's sources are verified against the public checksum database before use.
set -eu

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

# build_one <module-dir> <goos> <goarch> <abi-dir> <transport> <package>
build_one() {
  module=$1 goos=$2 goarch=$3 abi=$4 transport=$5 pkg=$6
  if [ -n "$abi" ]; then
    dir=$OUT/$abi
    # An executable despite the name (see header comment).
    name="lib$transport.so"
  else
    dir=$OUT
    name=$transport
    [ "$goos" = windows ] && name="$transport.exe"
  fi
  mkdir -p "$dir"
  (cd "$module" && GOOS=$goos GOARCH=$goarch CGO_ENABLED=0 go build -trimpath -buildvcs=false \
    ">-ldflags "-s -w -buildid= -checklinkname=0"< -o "$dir/$name" "$pkg")
  # Report the exact module versions that went into the binary.
  go version -m "$dir/$name" | awk '$1 == "dep" || $1 == "mod" {print}' | sed 's#^#    #'
}

# build_set <goos> <goarch> <abi-dir-or-empty>
build_set() {
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

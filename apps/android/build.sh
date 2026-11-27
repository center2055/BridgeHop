#!/usr/bin/env bash
# Builds the HopForge Android APK without Gradle:
#   Go sidecars + tor (Guardian AAR) + Rust engine -> aapt2 -> kotlinc -> d8 -> zipalign -> apksigner
#
# Needs: ANDROID_HOME (build-tools, platforms, ndk), a JDK (keytool), Go, Rust with the Android
# targets installed, and kotlinc (KOTLINC=/path/to/kotlinc/bin/kotlinc, or on PATH).
#
#   OUT=...            output directory (default build/android)
#   ABIS="arm64-v8a"   ABIs to package (default: arm64-v8a armeabi-v7a x86_64)
#   PT_DIR=...         reuse sidecars already built by pt-sidecars/build.sh
#   TOR_DIR=...        reuse <dir>/<abi>/libtor.so instead of extracting the AAR
#   TOR_URL=...        where to download the tor-android AAR when TOR_DIR is unset
#   KEYSTORE=...       signing key (created next to OUT on first use; its password lives in
#                      KEYSTORE.pass, mode 600, and is never passed on a command line)
#
# Nothing here needs root, Gradle, Python or a Google account.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
APP="$ROOT/apps/android"
OUT="${OUT:-$ROOT/build/android}"
WORK="$OUT/work"
ABIS="${ABIS:-arm64-v8a armeabi-v7a x86_64}"
ANDROID_API="${ANDROID_API:-24}"
VERSION_NAME="${VERSION_NAME:-0.1.0}"
TOR_URL="${TOR_URL:-}"
TOR_SHA256="${TOR_SHA256-9ffd3a8dab490733e1b29c7966922a9078989cd4ee386201a57d6c39d1403ddc}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"

SDK="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
[ -n "$SDK" ] || { echo "set ANDROID_HOME to the Android SDK" >&2; exit 1; }
NDK="${ANDROID_NDK_HOME:-$SDK/ndk/$(ls "$SDK/ndk" | sort -V | tail -n 1)}"
latest() { ls "$1" | sort -V | tail -n 1; }
BT="$SDK/build-tools/$(latest "$SDK/build-tools")"
ANDROID_JAR="$SDK/platforms/$(latest "$SDK/platforms")/android.jar"
NDK_BIN="$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin"
KOTLINC="${KOTLINC:-$(command -v kotlinc || true)}"

need() { command -v "$1" >/dev/null 2>&1 || { echo "missing: $1 ($2)" >&2; exit 1; }; }
need cargo "Rust with the android targets: rustup target add aarch64-linux-android"
need go "Go 1.26 or newer"
need keytool "a JDK (JAVA_HOME)"
need zip "zip"
need unzip "unzip"
need curl "curl"
[ -n "$KOTLINC" ] && [ -x "$KOTLINC" ] || { echo "missing: kotlinc (set KOTLINC=/path/to/kotlinc/bin/kotlinc)" >&2; exit 1; }
for tool in aapt2 d8 zipalign apksigner; do
    [ -x "$BT/$tool" ] || { echo "missing: $BT/$tool (Android build-tools)" >&2; exit 1; }
done
[ -f "$ANDROID_JAR" ] || { echo "missing: $ANDROID_JAR (Android platform)" >&2; exit 1; }
[ -x "$NDK_BIN/clang" ] || { echo "missing: $NDK_BIN/clang (Android NDK)" >&2; exit 1; }

rm -rf "$WORK"
mkdir -p "$WORK/tor" "$WORK/apk/lib" "$WORK/dex" "$OUT"

# 1. Pluggable transports and tor, one directory per ABI.
if [ -n "${PT_DIR:-}" ]; then
    PT="$PT_DIR"
else
    PT="$WORK/pt"
    # shellcheck disable=SC2046
    "$ROOT/pt-sidecars/build.sh" "$PT" $(for abi in $ABIS; do printf 'android/%s ' "$abi"; done)
fi
if [ -n "${TOR_DIR:-}" ]; then
    for abi in $ABIS; do mkdir -p "$WORK/tor/$abi"; cp "$TOR_DIR/$abi/libtor.so" "$WORK/tor/$abi/"; done
else
    [ -n "$TOR_URL" ] || { echo "set TOR_URL (the guardianproject tor-android AAR) or TOR_DIR" >&2; exit 1; }
    curl -fsSL -o "$WORK/tor.aar" "$TOR_URL"
    echo "$TOR_SHA256  $WORK/tor.aar" | sha256sum -c - > /dev/null
    (cd "$WORK/tor" && unzip -o -q "$WORK/tor.aar" 'jni/*')
    for abi in $ABIS; do mkdir -p "$WORK/tor/$abi"; cp "$WORK/tor/jni/$abi/libtor.so" "$WORK/tor/$abi/"; done
fi

# 2. The engine for each ABI: the same binary that is tested on the desktop, cross-compiled.
triple_of() {
    case "$1" in
        arm64-v8a) echo aarch64-linux-android ;;
        armeabi-v7a) echo armv7a-linux-androideabi ;;
        x86_64) echo x86_64-linux-android ;;
        *) echo "unsupported ABI: $1" >&2; exit 1 ;;
    esac
}
for abi in $ABIS; do
    triple="$(triple_of "$abi")"
    upper="$(printf '%s' "$triple" | tr 'a-z-' 'A-Z_')"
    export "CARGO_TARGET_${upper}_LINKER=$NDK_BIN/$triple-$ANDROID_API-clang"
    lower="$(printf "%s" "$triple" | tr "A-Z-" "a-z_")"
    export "CC_${lower}=$NDK_BIN/$triple-$ANDROID_API-clang"
    export "AR_${lower}=$NDK_BIN/llvm-ar"
    export PATH="$NDK_BIN:$PATH"
    (cd "$ROOT" && cargo build --release --target "$triple" -p hf-engine)
done

# 3. Resources, the bundled page and the Kotlin shell.
"$BT/aapt2" compile --dir "$APP/res" -o "$WORK/res.zip"
"$BT/aapt2" link -o "$WORK/base.apk" -I "$ANDROID_JAR" --manifest "$APP/AndroidManifest.xml" \
    -R "$WORK/res.zip" -A "$APP/assets" --auto-add-overlay
mapfile -t sources < <(find "$APP/src" -name '*.kt' | sort)
"$KOTLINC" -jvm-target 1.8 -classpath "$ANDROID_JAR" -d "$WORK/classes.jar" "${sources[@]}"
"$BT/d8" --release --min-api "$ANDROID_API" --lib "$ANDROID_JAR" --output "$WORK/dex" "$WORK/classes.jar"

# 4. Assemble: dex plus the native executables (uncompressed, page-aligned).
cp "$WORK/base.apk" "$WORK/unsigned.apk"
(cd "$WORK/dex" && zip -q -X "$WORK/unsigned.apk" classes.dex)
for abi in $ABIS; do
    lib="$WORK/apk/lib/$abi"
    mkdir -p "$lib"
    cp "$CARGO_TARGET_DIR/$(triple_of "$abi")/release/hf-engine" "$lib/libhopforge.so"
    cp "$WORK/tor/$abi/libtor.so" "$lib/libtor.so"
    for transport in lyrebird snowflake webtunnel conjure; do
        [ -f "$PT/$abi/lib$transport.so" ] || { echo "missing sidecar $PT/$abi/lib$transport.so" >&2; exit 1; }
        cp "$PT/$abi/lib$transport.so" "$lib/lib$transport.so"
    done
    (cd "$WORK/apk" && zip -q -X -0 "$WORK/unsigned.apk" "lib/$abi"/*.so)
done
"$BT/zipalign" -P 16 -f 4 "$WORK/unsigned.apk" "$WORK/aligned.apk"

# 5. Sign with a key that is generated here and never committed.
KS="${KEYSTORE:-$OUT/hopforge-signing.p12}"
if [ ! -f "$KS" ]; then
    (umask 077; head -c 24 /dev/urandom | base64 | tr -d '/+=\n' > "$KS.pass")
    keytool -genkeypair -keystore "$KS" -storetype PKCS12 -alias hopforge -keyalg RSA -keysize 4096 \
        -validity 10000 -storepass:file "$KS.pass" -dname "CN=HopForge Local Build, O=HopForge" > /dev/null 2>&1
fi
APK="$OUT/HopForge-$VERSION_NAME.apk"
"$BT/apksigner" sign --ks "$KS" --ks-key-alias hopforge --ks-pass "file:$KS.pass" \
    --v1-signing-enabled false --v2-signing-enabled true --v3-signing-enabled true \
    --out "$APK" "$WORK/aligned.apk"
"$BT/apksigner" verify --verbose --print-certs "$APK"
echo "APK: $APK ($(stat -c %s "$APK") bytes)"

#!/usr/bin/env bash
# Builds the HopForge Android APK without Gradle:
#   Go sidecars + tor (Guardian AAR) + Rust engine -> aapt2 -> kotlinc -> d8 -> zipalign -> apksigner -> verify.sh
#
# Needs: ANDROID_HOME (build-tools and a platform; an NDK unless CC_WRAPPERS_DIR is set), a JDK
# (keytool), Go, Rust with the Android targets installed, and kotlinc (KOTLINC=/path/to/kotlinc/bin/kotlinc,
# or on PATH).
#
#   OUT=...              output directory (default build/android)
#   ABIS="arm64-v8a"     ABIs to package (default: arm64-v8a)
#   PT_DIR=...           reuse sidecars already built by pt-sidecars/build.sh
#   TOR_DIR=...          reuse <dir>/<abi>/libtor.so instead of downloading the AAR
#   TOR_URL=...          where to download the tor-android AAR when TOR_DIR is unset (pinned below)
#   KEYSTORE=...         signing key (created next to OUT on first use; its password lives in
#                        KEYSTORE.pass, mode 600, and is never passed on a command line)
#   CC_WRAPPERS_DIR=...  use ready-made cc-<triple> scripts instead of the NDK (Termux compiles
#                        for its own CPU with its own clang); ANDROID_AR must then name an archiver
#   SKIP_VERIFY=1        do not run verify.sh at the end (not recommended)
#
# Nothing here needs root, Gradle, Python or a Google account.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
APP="$ROOT/apps/android"
OUT="${OUT:-$ROOT/build/android}"
WORK="$OUT/work"
ABIS="${ABIS:-arm64-v8a}"
ANDROID_API="${ANDROID_API:-24}"
VERSION_NAME="${VERSION_NAME:-0.1.0}"
VERSION_CODE="${VERSION_CODE:-1}"
# tor 0.4.9.17.1 for Android, Guardian Project's build (BSD-3-Clause), from their Maven repository
# and pinned by sha256. Guardian Project publishes no checksum file for it; the value is the one
# recorded when this line was written (see docs/EVIDENCE.md).
TOR_URL="${TOR_URL:-https://raw.githubusercontent.com/guardianproject/gpmaven/master/info/guardianproject/tor-android/0.4.9.17.1/tor-android-0.4.9.17.1.aar}"
TOR_SHA256="${TOR_SHA256-74d007c4ed6b5ff4fdadf3e2c3113fe7d7b4026717e633230cd8936a141a9ac3}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"

triple_of() {
    case "$1" in
        arm64-v8a) echo aarch64-linux-android ;;
        armeabi-v7a) echo armv7a-linux-androideabi ;;
        x86_64) echo x86_64-linux-android ;;
        *) echo "unsupported ABI: $1" >&2; return 1 ;;
    esac
}
latest() { ls "$1" | sort -V | tail -n 1; }
need() { command -v "$1" >/dev/null 2>&1 || { echo "missing: $1 ($2)" >&2; exit 1; }; }

SDK="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
[ -n "$SDK" ] || { echo "set ANDROID_HOME to the Android SDK" >&2; exit 1; }
BT="${BT:-$SDK/build-tools/$(latest "$SDK/build-tools")}"
ANDROID_JAR="${ANDROID_JAR:-$SDK/platforms/$(latest "$SDK/platforms")/android.jar}"
KOTLINC="${KOTLINC:-$(command -v kotlinc || true)}"
ZIPALIGN="${ZIPALIGN:-$BT/zipalign}"
export READELF="${READELF:-$(command -v readelf || true)}"

triples=()
for abi in $ABIS; do triples+=("$(triple_of "$abi")"); done

need cargo "Rust with the android targets: rustup target add aarch64-linux-android"
need go "Go 1.26 or newer"
need keytool "a JDK (JAVA_HOME)"
need zip "zip"
need unzip "unzip"
need curl "curl"
need sha256sum "coreutils"
for tool in aapt2 d8 apksigner; do
    [ -x "$BT/$tool" ] || { echo "missing: $BT/$tool (Android build-tools)" >&2; exit 1; }
done
[ -f "$ANDROID_JAR" ] || { echo "missing: $ANDROID_JAR (Android platform)" >&2; exit 1; }
[ -n "$KOTLINC" ] && [ -x "$KOTLINC" ] || { echo "missing: kotlinc (set KOTLINC=/path/to/kotlinc/bin/kotlinc)" >&2; exit 1; }
[ -n "$READELF" ] || { echo "missing: readelf (binutils)" >&2; exit 1; }

# The compiler: either the NDK's clang (driven through the wrappers that cc-wrappers.sh writes)
# or the caller's own cc-<triple> scripts.
if [ -z "${CC_WRAPPERS_DIR:-}" ]; then
    NDK="${ANDROID_NDK_HOME:-$SDK/ndk/$(latest "$SDK/ndk")}"
    NDK_BIN="${NDK_BIN:-$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin}"
    [ -x "$NDK_BIN/clang" ] || { echo "missing: $NDK_BIN/clang (Android NDK)" >&2; exit 1; }
    AR="${ANDROID_AR:-$NDK_BIN/llvm-ar}"
else
    WRAPPERS="$CC_WRAPPERS_DIR"
    # Where the linker driver looks for ld.lld; Termux has it on PATH already.
    NDK_BIN="${NDK_BIN:-}"
    AR="${ANDROID_AR:-CC_WRAPPERS_DIR is set, so ANDROID_AR must name llvm-ar}"
    [ -x "$AR" ] || { echo "$AR" >&2; exit 1; }
fi

rm -rf "$WORK"
mkdir -p "$WORK/tor" "$WORK/apk/lib" "$WORK/dex" "$OUT"

# 1. Pluggable transports and tor, one directory per ABI.
if [ -n "${PT_DIR:-}" ]; then
    PT="$PT_DIR"
else
    PT="$WORK/pt"
    "$ROOT/pt-sidecars/build.sh" "$PT" $(for abi in $ABIS; do printf 'android/%s ' "$abi"; done)
fi
if [ -n "${TOR_DIR:-}" ]; then
    for abi in $ABIS; do mkdir -p "$WORK/tor/$abi"; cp "$TOR_DIR/$abi/libtor.so" "$WORK/tor/$abi/"; done
else
    curl -fsSL --retry 5 --connect-timeout 20 -o "$WORK/tor.aar" "$TOR_URL"
    echo "$TOR_SHA256  $WORK/tor.aar" | sha256sum -c - > /dev/null
    (cd "$WORK/tor" && unzip -o -q "$WORK/tor.aar" 'jni/*')
    for abi in $ABIS; do mkdir -p "$WORK/tor/$abi"; cp "$WORK/tor/jni/$abi/libtor.so" "$WORK/tor/$abi/"; done
fi

# 2. The engine for each ABI: the same code that is tested on the desktop, cross-compiled.
if [ -z "${CC_WRAPPERS_DIR:-}" ]; then
    WRAPPERS="$WORK"
    "$APP/cc-wrappers.sh" "$NDK_BIN" "$WORK" "$ANDROID_API" "${triples[@]}"
fi
# 16 KB pages: Android 15+ devices with a 16 KB page size need LOAD segments aligned to 16384.
# CARGO_ENCODED_RUSTFLAGS separates arguments with the 0x1f byte (not spaces).
flags="${CARGO_ENCODED_RUSTFLAGS:-}"
[ -n "$flags" ] && flags="$flags"$'\x1f'
export CARGO_ENCODED_RUSTFLAGS="${flags}-C"$'\x1f'"link-arg=-Wl,-z,max-page-size=16384"
for triple in "${triples[@]}"; do
    upper="$(printf '%s' "$triple" | tr 'a-z-' 'A-Z_')"
    lower="$(printf '%s' "$triple" | tr 'A-Z-' 'a-z_')"
    export "CARGO_TARGET_${upper}_LINKER=$WRAPPERS/cc-$triple"
    export "CC_${lower}=$WRAPPERS/cc-$triple"
    export "AR_${lower}=$AR"
    if [ -n "$NDK_BIN" ]; then export PATH="$NDK_BIN:$PATH"; fi
    (cd "$ROOT" && cargo build --release --target "$triple" -p hf-engine)
done

# 3. Resources, the bundled page and the Kotlin shell.
"$BT/aapt2" compile --dir "$APP/res" -o "$WORK/res.zip"
"$BT/aapt2" link -o "$WORK/base.apk" -I "$ANDROID_JAR" --manifest "$APP/AndroidManifest.xml" \
    --version-code "$VERSION_CODE" --version-name "$VERSION_NAME" \
    -R "$WORK/res.zip" -A "$APP/assets" --auto-add-overlay
mapfile -t sources < <(find "$APP/src" -name '*.kt' | sort)
"$KOTLINC" -jvm-target 1.8 -classpath "$ANDROID_JAR" -d "$WORK/classes.jar" "${sources[@]}"
"$BT/d8" --release --min-api "$ANDROID_API" --lib "$ANDROID_JAR" --output "$WORK/dex" "$WORK/classes.jar"

# 4. Assemble: dex plus the native executables (uncompressed, page-aligned).
cp "$WORK/base.apk" "$WORK/unsigned.apk"
(cd "$WORK/dex" && zip -q -X "$WORK/unsigned.apk" classes.dex)
for abi in $ABIS; do
    triple="$(triple_of "$abi")"
    lib="$WORK/apk/lib/$abi"
    mkdir -p "$lib"
    cp "$CARGO_TARGET_DIR/$triple/release/hf-engine" "$lib/libhopforge.so"
    cp "$WORK/tor/$abi/libtor.so" "$lib/libtor.so"
    for transport in lyrebird snowflake webtunnel conjure; do
        [ -f "$PT/$abi/lib$transport.so" ] || { echo "missing sidecar $PT/$abi/lib$transport.so" >&2; exit 1; }
        cp "$PT/$abi/lib$transport.so" "$lib/lib$transport.so"
    done
    (cd "$WORK/apk" && zip -q -X -0 "$WORK/unsigned.apk" "lib/$abi"/*.so)
done
if [ -x "$ZIPALIGN" ]; then
    "$ZIPALIGN" -P 16 -f 4 "$WORK/unsigned.apk" "$WORK/aligned.apk"
    TO_SIGN="$WORK/aligned.apk"
else
    echo "note: zipalign not found; signing the APK unaligned (it still installs: extractNativeLibs=true)"
    TO_SIGN="$WORK/unsigned.apk"
fi

# 5. Sign with a key that is generated here and never committed.
KS="${KEYSTORE:-$OUT/hopforge-signing.p12}"
if [ ! -f "$KS" ]; then
    (umask 077; head -c 24 /dev/urandom | base64 | tr -d '/+=\n' > "$KS.pass")
    # A generic name on purpose: nothing in the APK or the key says who built it.
    keytool -genkeypair -keystore "$KS" -storetype PKCS12 -alias hopforge -keyalg RSA -keysize 4096 \
        -validity 10000 -storepass:file "$KS.pass" -dname "CN=HopForge Local Build, O=HopForge" > /dev/null
fi
APK="$OUT/HopForge-$VERSION_NAME.apk"
"$BT/apksigner" sign --ks "$KS" --ks-key-alias hopforge --ks-pass "file:$KS.pass" \
    --v1-signing-enabled false --v2-signing-enabled true --v3-signing-enabled true \
    --v4-signing-enabled false --alignment-preserved true --out "$APK" "$TO_SIGN"
"$BT/apksigner" verify --verbose --print-certs "$APK"

if [ -z "${SKIP_VERIFY:-}" ]; then
    READELF="$READELF" ZIPALIGN="${ZIPALIGN:-}" bash "$APP/verify.sh" "$APK" "$BT" "$ABIS"
fi
echo "APK: $APK ($(stat -c %s "$APK") bytes)"

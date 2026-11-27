#!/data/data/com.termux/files/usr/bin/bash
# HopForge in Termux: installs the toolchain, builds, signs and verifies the APK on the phone,
# records the evidence, and pushes that evidence to the repository (only when a token is given).
# No root, no Python, no Gradle, no questions.
#
#   pkg update -y && pkg install -y git \
#     && git clone --branch hopforge https://github.com/ysa-py/BridgeHop.git \
#     && bash BridgeHop/scripts/termux-build.sh
#
# Before it builds, the script proves that this phone's compiler can produce a program that runs
# outside Termux: a C, a Go (cgo) and a Rust hello world, each linked the way the app's own
# programs are, each checked for Android's interpreter, libc and the absence of a search path
# into Termux's directories. If that fails, nothing is built and the error says why.
#
# The signing key is created on the first run and reused after: keep it, or the next APK cannot
# be installed over the previous one. It never leaves the phone unless BACKUP_KEY=1 is set, which
# copies it to the shared Downloads folder (needs `termux-setup-storage` once).
#
# Push (optional): export GH_TOKEN with a fine-grained token for ysa-py/BridgeHop (Contents: read
# and write). Only docs/termux-builds/<timestamp>-<abi>.md is committed. Nothing else about the
# phone is written there: no serial number, no location, and no model unless REPORT_DEVICE=1.
#
#   WORK=...           where everything is kept (default: ~/hopforge)
#   HOPFORGE_FORCE=1   go on although less than 6 GB is free (not recommended)
#   BACKUP_KEY=1       copy the signing key to ~/storage/downloads
#   REPORT_DEVICE=1    name the device model in the evidence file
#
# UNVERIFIED: written from the desktop build of the same steps; not yet run on a Termux device.
set -euo pipefail

PREFIX="${PREFIX:-/data/data/com.termux/files/usr}"
WORK="${WORK:-$HOME/hopforge}"
BRANCH="${BRANCH:-hopforge}"
OUT="$WORK/out"
LOGS="$WORK/logs"
SDK="$WORK/android-sdk"
KEYS="$WORK/signing"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
LOG="$LOGS/build-$STAMP.log"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

step() { printf '\n==> %s\n' "$*"; }
die() { echo "FAILED: $*" >&2; echo "The complete log is in $LOG" >&2; exit 1; }
have() { command -v "$1" > /dev/null 2>&1; }

[ -x "$PREFIX/bin/pkg" ] || { echo "Run this inside the Termux app: bash scripts/termux-build.sh" >&2; exit 1; }
mkdir -p "$OUT" "$LOGS" "$SDK" "$KEYS"
exec > >(tee -a "$LOG") 2>&1
trap 'echo "FAILED at line $LINENO of scripts/termux-build.sh (exit $?); see $LOG" >&2' ERR
termux-wake-lock > /dev/null 2>&1 || true

# ---- this phone -------------------------------------------------------------------------
case "$(uname -m)" in
    aarch64) abi=arm64-v8a;      triple=aarch64-linux-android;    goarch=arm64;  interp=/system/bin/linker64 ;;
    armv7l|armv8l) abi=armeabi-v7a; triple=armv7a-linux-androideabi; goarch=arm;   interp=/system/bin/linker ;;
    x86_64)  abi=x86_64;         triple=x86_64-linux-android;     goarch=amd64;  interp=/system/bin/linker64 ;;
    *) die "unsupported CPU: $(uname -m)" ;;
esac
free_kb="$(df -Pk "$HOME" | awk 'NR == 2 { print $4 }')"
if [ "$free_kb" -lt 6000000 ] && [ -z "${HOPFORGE_FORCE:-}" ]; then
    die "only $((free_kb / 1024)) MB free; the toolchain, sources and build files need about 6 GB (HOPFORGE_FORCE=1 goes on anyway)"
fi
echo "HopForge build in Termux: $abi ($triple), $((free_kb / 1024)) MB free"

# ---- packages (names and versions are those of the Termux repository) --------------------
step "Termux packages (about 700 MB to download; this is the slow part)"
pkg update -y || die "'pkg update' failed. Run 'termux-change-repo', pick another mirror, then run this script again"
pkg install -y git golang rust clang llvm lld ndk-sysroot libandroid-stub libcompiler-rt \
    openjdk-21 kotlin aapt2 d8 apksigner zip unzip curl
for tool in git go cargo rustc clang llvm-ar java keytool kotlinc aapt2 d8 apksigner zip unzip curl sha256sum; do
    have "$tool" || die "$tool was not installed; is the Termux repository reachable?"
done
[ -x "$PREFIX/bin/sh" ] || die "$PREFIX/bin/sh is missing (the compiler drivers are started by it)"
READELF="$(command -v llvm-readelf || command -v readelf || true)"
[ -n "$READELF" ] || die "no readelf (llvm-readelf) found"

# ---- android.jar: the platform the app is compiled against (pinned, as in bootstrap.sh) --
step "android.jar (Android platform 36)"
JAR="$SDK/platforms/android-36/android.jar"
if [ ! -f "$JAR" ]; then
    mkdir -p "$SDK/platforms/android-36" "$WORK/tmp"
    curl -fL --retry 5 --retry-all-errors --connect-timeout 30 -o "$WORK/tmp/platform.zip.part" \
        "https://dl.google.com/android/repository/platform-36_r02.zip"
    [ "$(stat -c %s "$WORK/tmp/platform.zip.part")" = 65878410 ] || die "platform-36_r02.zip has another size than pinned"
    echo "2c1a80dd4d9f7d0e6dd336ec603d9b5c55a6f576  $WORK/tmp/platform.zip.part" | sha1sum -c - > /dev/null \
        || die "platform-36_r02.zip does not match the pinned sha1"
    rm -rf "$WORK/tmp/platform" && mkdir -p "$WORK/tmp/platform"
    unzip -q "$WORK/tmp/platform.zip.part" -d "$WORK/tmp/platform"
    grep -q '^AndroidVersion.ApiLevel=36$' "$WORK/tmp/platform/android-36/source.properties" \
        || die "the platform zip is not API 36"
    cp "$WORK/tmp/platform/android-36/android.jar" "$JAR"
    rm -rf "$WORK/tmp/platform" "$WORK/tmp/platform.zip.part"
fi

# ---- proof that this compiler makes programs that run outside Termux ----------------------
step "compiler check"
# Termux's clang sends every program to the libraries of its own directory at run time unless it
# is told not to; a program inside the app must only use Android's own libraries.
if echo 'int main(void){return 0;}' | "$PREFIX/bin/clang" -fno-termux-rpath -x c -o "$WORK/probe" - 2> /dev/null; then
    export HOPFORGE_CC_FLAGS=-fno-termux-rpath
fi
rm -f "$WORK/probe"
CCW="$WORK/smoke"
rm -rf "$CCW" && mkdir -p "$CCW"
# The same driver the build uses (see apps/android/cc-wrappers.sh).
sh "$ROOT/apps/android/cc-wrappers.sh" "$PREFIX/bin" "$CCW" 24 "$PREFIX/bin/sh" "$triple"
CC="$CCW/cc-$triple"
system_libs=' libc.so libm.so libdl.so liblog.so libz.so libandroid.so libnetd_client.so '
elf_ok() { # elf_ok <program>
    local dyn bad=""
    dyn="$("$READELF" -d "$1")"
    has() { grep -qE -- "$1" <<<"$dyn"; }
    has "NEEDED.*\[libc\.so\]" || die "$1 does not use Android's libc"
    has '\((RPATH|RUNPATH)\)' && die "$1 carries a search path into this phone's directories: $(grep -E '\((RPATH|RUNPATH)\)' <<<"$dyn")"
    for lib in $(grep NEEDED <<<"$dyn" | sed 's/.*\[\(.*\)\]$/\1/'); do
        case "$system_libs" in *" $lib "*) ;; *) bad="$bad $lib" ;; esac
    done
    [ -z "$bad" ] || die "$1 needs libraries that are not on every Android device:$bad"
    grep -q "Requesting program interpreter: $interp\]" <<<"$("$READELF" -lW "$1")" || die "$1 does not ask for $interp"
    echo "  ok    $1"
}
echo 'int main(void){return 0;}' | "$CC" -x c -o "$CCW/hello-c" -
elf_ok "$CCW/hello-c"
cat > "$CCW/go.go" <<'EOF'
package main

// #include <stdlib.h>
import "C"

func main() { println(*C.CString("x") == 'x') }
EOF
(cd "$CCW" && GOOS=android GOARCH="$goarch" CGO_ENABLED=1 CC="$CC" go build -o hello-go go.go)
elf_ok "$CCW/hello-go"
printf 'fn main() { println!("ok"); }\n' > "$CCW/rust.rs"
rustc --target "$triple" -C linker="$CC" -o "$CCW/hello-rust" "$CCW/rust.rs"
elf_ok "$CCW/hello-rust"
rm -rf "$CCW"

# ---- the build (the same script as on the desktop, with this phone's own compiler) --------
step "building the APK (long: Go transports, the Rust engine, Kotlin, packaging, signing, checks)"
export BT="$PREFIX/bin" ANDROID_JAR="$JAR" NDK_BIN="$PREFIX/bin" \
    ANDROID_AR="$(command -v llvm-ar)" KOTLINC="$(command -v kotlinc)" READELF \
    ABIS="$abi" OUT KEYSTORE="$KEYS/hopforge-signing.p12"
export CARGO_TARGET_DIR="$WORK/cargo-target" CARGO_BUILD_JOBS=2 GOFLAGS=-p=2   # two at a time: Android kills processes that use too much memory
bash "$ROOT/apps/android/build.sh"
APK="$OUT/HopForge-${VERSION_NAME:-0.1.0}.apk"
SHA="$(sha256sum "$APK" | cut -d' ' -f1)"
CERT="$("$BT/apksigner" verify --print-certs "$APK" | sed -n 's/^Signer #1 certificate SHA-256 digest: //p')"

# ---- evidence: what was built, on what, with which tools --------------------------------
step "evidence"
ANDROID="$(getprop ro.build.version.release 2> /dev/null || echo unknown) (API $(getprop ro.build.version.sdk 2> /dev/null || echo unknown))"
MODEL="not recorded (REPORT_DEVICE=1 adds it)"
[ -z "${REPORT_DEVICE:-}" ] || MODEL="$(getprop ro.product.manufacturer 2> /dev/null || echo unknown) $(getprop ro.product.model 2> /dev/null || echo unknown)"
mkdir -p "$ROOT/docs/termux-builds"
{
    echo "# HopForge built in Termux: $abi, $STAMP"
    echo
    echo "Built on the phone itself with scripts/termux-build.sh. Android $ANDROID; device: $MODEL."
    echo "Nothing here says where the phone is or who owns it."
    echo
    echo "| Tool | Version |"
    echo "| --- | --- |"
    for tool in "go version" "rustc --version" "cargo --version" "clang --version" "java -version" "kotlinc -version" "d8 --version"; do
        echo "| ${tool%% *} | $("$tool" 2>&1 | head -n 1 | cut -c1-120) |"
    done
    echo
    echo "APK: \`$(basename "$APK")\`, $(stat -c %s "$APK") bytes, sha256 \`$SHA\`"
    echo
    echo "Signer certificate sha256: \`$CERT\`"
    echo
    echo "Compiler check before the build: C, Go (cgo) and Rust programs linked with \`--target=$triple\`24${HOPFORGE_CC_FLAGS:+ and $HOPFORGE_CC_FLAGS}: Android's interpreter and libc only, no search path into Termux."
    echo
    echo '```'
    grep -E '^  (PASS|FAIL|note)|^verify:|^SHA-256:|^  signer' "$LOG"
    echo '```'
} > "$ROOT/docs/termux-builds/$STAMP-$abi.md"

# ---- the key is the only thing that cannot be made again: offer a copy --------------------
if [ -n "${BACKUP_KEY:-}" ] && [ -d "$HOME/storage/downloads" ]; then
    mkdir -p "$HOME/storage/downloads/hopforge-signing"
    cp "$KEYSTORE/hopforge-signing.p12" "$KEYSTORE/hopforge-signing.p12.pass" "$HOME/storage/downloads/hopforge-signing/"
    echo "signing key copied to Downloads/hopforge-signing (keep it private)"
fi
if [ -d "$HOME/storage/downloads" ]; then
    cp "$APK" "$HOME/storage/downloads/"
    echo "APK copied to Downloads/$(basename "$APK")"
fi

# ---- push the evidence, only with a token from the environment --------------------------
if [ -n "${GH_TOKEN:-}${GITHUB_TOKEN:-}" ]; then
    step "pushing the evidence to $BRANCH"
    token="${GH_TOKEN:-${GITHUB_TOKEN:-}}"
    case "$(git -C "$ROOT" remote get-url origin)" in
        https://github.com/ysa-py/BridgeHop|https://github.com/ysa-py/BridgeHop.git) ;;
        *) die "the checkout's remote is not https://github.com/ysa-py/BridgeHop" ;;
    esac
    askpass="$(mktemp)"
    cat > "$askpass" <<'EOF'
#!/data/data/com.termux/files/usr/bin/sh
case "$1" in
    *sername*) echo x-access-token ;;
    *) echo "$HOPFORGE_PUSH_TOKEN" ;;
esac
EOF
    chmod 700 "$askpass"
    git -C "$ROOT" add docs/termux-builds
    git -C "$ROOT" -c user.name="${GIT_NAME:-hopforge-builder}" \
        -c user.email="${GIT_EMAIL:-hopforge-builder@users.noreply.github.com}" \
        commit -q -m "HopForge: build evidence from Termux ($abi, $STAMP)" || echo "nothing new to commit"
    HOPFORGE_PUSH_TOKEN="$token" GIT_ASKPASS="$askpass" GIT_TERMINAL_PROMPT=0 git -C "$ROOT" \
        -c credential.helper= push origin "HEAD:$BRANCH" || { rm -f "$askpass"; die "the push failed; the APK is still in $OUT"; }
    rm -f "$askpass"
    unset token
    echo "pushed: docs/termux-builds/$STAMP-$abi.md"
fi

step "done"
echo "APK:        $APK"
echo "sha256:     $SHA"
echo "signer:     $CERT"
echo "signing key: $KEYS/hopforge-signing.p12 (keep it; BACKUP_KEY=1 copies it to Downloads)"
echo "log:        $LOG"
if have termux-open; then
    termux-open "$APK" > /dev/null 2>&1 || true
    echo "Android is asking whether to install it. If it refuses: Settings > Apps > Special access > Install unknown apps > Termux > Allow."
fi

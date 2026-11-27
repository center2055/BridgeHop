#!/data/data/com.termux/files/usr/bin/bash
# HopForge on Termux: installs what is missing, builds the APK on the phone, commits and pushes.
#   1) Termux packages          2) android.jar + tool links       3) apps/android/build.sh
#   4) git commit, push to ysa-py/BridgeHop (branch "hopforge") and a pull request with gh
#
# No root, no Python, no Gradle. Before the push:  export GITHUB_TOKEN=<fine-grained PAT,
# Contents + Pull requests: write, on ysa-py/BridgeHop>. The token is read from the environment
# only: it is never written to disk and never appears in a URL.
#
# tor is not built here: give TOR_URL (the tor-android 0.4.9.17 AAR; its sha256 is pinned in
# build.sh) or TOR_DIR (a directory holding <abi>/libtor.so).
#
# UNVERIFIED: this script was written from a desktop build of the same steps and has not been
# run on a Termux device. If a step fails, its output says which package or tool is missing.
set -euo pipefail

REPO="${REPO:-https://github.com/ysa-py/BridgeHop.git}"
WORK="${WORK:-$HOME/hopforge}"
BRANCH="${BRANCH:-hopforge}"
SDK="$WORK/android-sdk"
TOR_URL="${TOR_URL:-}"
TOR_DIR="${TOR_DIR:-}"
export TOR_URL TOR_DIR

step() { printf '\n==> %s\n' "$*"; }
have() { command -v "$1" > /dev/null 2>&1; }

step "Termux packages"
pkg update -y
pkg install -y git golang rust binutils clang llvm unzip zip curl gh
have keytool || pkg install -y openjdk-21
have kotlinc || pkg install -y kotlin
for tool in aapt2 d8 apksigner zipalign; do
    have "$tool" || pkg install -y "$tool"
done
for tool in cargo go kotlinc keytool aapt2 d8 apksigner zipalign clang llvm-ar; do
    have "$tool" || { echo "missing: $tool — install it with 'pkg install' and run again" >&2; exit 1; }
done
rustup target add aarch64-linux-android 2> /dev/null || true

step "sources"
mkdir -p "$WORK"
if [ ! -d "$WORK/BridgeHop/.git" ]; then git clone "$REPO" "$WORK/BridgeHop"; fi
cd "$WORK/BridgeHop"
git checkout -B "$BRANCH"

step "android.jar and tool links"
BIN="$SDK/ndk/hopforge/toolchains/llvm/prebuilt/linux-x86_64/bin"
mkdir -p "$SDK/build-tools/termux" "$SDK/platforms" "$BIN"
if [ ! -f "$SDK/platforms/android-36/android.jar" ]; then
    curl -fsSL -o "$WORK/platform.zip" "https://dl.google.com/android/repository/platform-36_r02.zip"
    unzip -q -o "$WORK/platform.zip" -d "$WORK/platform"
    mkdir -p "$SDK/platforms/android-36"
    cp "$WORK/platform/android-36/android.jar" "$SDK/platforms/android-36/android.jar"
fi
for tool in aapt2 d8 apksigner zipalign; do
    ln -sf "$(command -v "$tool")" "$SDK/build-tools/termux/$tool"
done
# The phone compiles for its own CPU, so the native compiler is also the "cross" compiler.
for triple in aarch64-linux-android armv7a-linux-androideabi x86_64-linux-android; do
    printf '#!/bin/sh\nexec clang --target=%s "$@"\n' "$triple" > "$BIN/$triple-clang"
    printf '#!/bin/sh\nexec clang --target=%s "$@"\n' "$triple" > "$BIN/cc-$triple"
    for api in 21 24; do
        printf '#!/bin/sh\nexec clang --target=%s%s "$@"\n' "$triple" "$api" > "$BIN/$triple-$api-clang"
    done
done
ln -sf "$(command -v clang)" "$BIN/clang"
ln -sf "$(command -v llvm-ar)" "$BIN/llvm-ar"

step "build"
export ANDROID_HOME="$SDK"
export ANDROID_NDK_HOME="$SDK/ndk/hopforge"
OUT="$WORK/out" ABIS="${ABIS:-arm64-v8a}" KOTLINC="$(command -v kotlinc)" bash apps/android/build.sh

step "commit"
git add -A
git -c user.name="${GIT_NAME:-hopforge-builder}" \
    -c user.email="${GIT_EMAIL:-hopforge-builder@users.noreply.github.com}" \
    commit -m "HopForge: Android build from Termux $(date -u +%Y-%m-%dT%H:%M:%SZ)"

step "push and pull request"
if [ -z "${GITHUB_TOKEN:-}" ]; then
    echo "APK is ready in $WORK/out. To push: export GITHUB_TOKEN=... and run this script again." >&2
    exit 0
fi
git remote set-url origin "$REPO"
git -c credential.helper= \
    -c 'credential.helper=!f(){ echo username=x-access-token; echo "password=$GITHUB_TOKEN"; }; f' \
    push -u origin "$BRANCH"
GH_TOKEN="$GITHUB_TOKEN" gh pr create --repo ysa-py/BridgeHop --base main --head "$BRANCH" \
    --title "HopForge: Rust core, verified bridges, Android app" \
    --body "Built on Termux by scripts/termux-build.sh." || true
echo "APK: $WORK/out/HopForge-*.apk"

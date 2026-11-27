#!/usr/bin/env bash
# Puts the toolchain built by scripts/sandbox/bootstrap.sh on PATH. Source it, with the same
# toolchain directory:
#   . scripts/sandbox/env.sh /path/to/toolchain
# Every cache directory is kept inside the toolchain too, so nothing is written to $HOME.
# Usage errors must stop the caller whether this file is sourced or executed.
stop() { echo "usage: . scripts/sandbox/env.sh <toolchain-dir>" >&2; return 1 2> /dev/null || exit 1; }
[ "$#" -eq 1 ] && [ -d "$1" ] || stop
TC=$(cd "$1" && pwd) || stop

export HOPFORGE_TOOLCHAIN="$TC"
export ANDROID_HOME="$TC/android"
export ANDROID_NDK_HOME="$TC/android/ndk/r30"
export GOROOT="$TC/go"
export GOPATH="$TC/gopath"
export GOTOOLCHAIN=local      # a go.mod asking for another Go version is an error, not a download
export RUSTUP_HOME="$TC/rust"
export CARGO_HOME="$TC/cargo"
export PATH="$TC/go/bin:$TC/cargo/bin:$TC/android/build-tools/36.0.0:$TC/kotlin/bin:$PATH"
export D8="$TC/d8/d8"             # the newer dexer (see bootstrap.sh); apps/android/build.sh reads it
unset GOFLAGS

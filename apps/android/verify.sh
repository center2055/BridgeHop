#!/usr/bin/env bash
# Checks a finished HopForge APK the way a careful user would before installing it, and exits
# with 1 if anything is off:
#   signature (v2 and v3, exactly one signer), zip alignment, package name, SDK levels,
#   manifest flags, permissions, exported components, the ABIs, and the ELF headers of every
#   native executable (16 KB LOAD alignment, Android's interpreter, only system libraries,
#   no text relocations).
#
#   apps/android/verify.sh <apk> <build-tools-dir> "<abi>..."
# READELF (default: readelf on PATH) and ZIPALIGN (default: <build-tools-dir>/zipalign) may be
# set in the environment.
set -euo pipefail

[ "$#" -eq 3 ] || { echo "usage: $0 <apk> <build-tools-dir> \"<abi>...\"" >&2; exit 2; }
APK=$1
BT=$2
ABIS=$3
READELF="${READELF:-readelf}"
ZIPALIGN="${ZIPALIGN:-$BT/zipalign}"

fails=0
pass() { echo "  PASS  $*"; }
fail() { echo "  FAIL  $*"; fails=$((fails + 1)); }
note() { echo "  note  $*"; }
check() { # check <description> <command...>: passes when the command succeeds
    local what=$1
    shift
    if "$@" > /dev/null 2>&1; then pass "$what"; else fail "$what"; fi
}

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "APK: $APK ($(stat -c %s "$APK") bytes)"
echo "SHA-256: $(sha256sum "$APK" | cut -d' ' -f1)"

# ---- signature -------------------------------------------------------------------------
sig="$("$BT/apksigner" verify --verbose --print-certs "$APK" 2>&1)" || {
    fail "apksigner verify (see below)"
    echo "$sig" | sed 's/^/        /'
}
check "signed with APK Signature Scheme v2" grep -q '^Verified using v2 scheme (APK Signature Scheme v2): true' <<<"$sig"
check "signed with APK Signature Scheme v3" grep -q '^Verified using v3 scheme (APK Signature Scheme v3): true' <<<"$sig"
check "exactly one signer" grep -q '^Number of signers: 1$' <<<"$sig"
cert="$(grep -m1 'certificate SHA-256 digest' <<<"$sig" | sed 's/.*digest: //')"
echo "  signer certificate SHA-256: $cert"

# ---- alignment --------------------------------------------------------------------------
if [ -x "$ZIPALIGN" ]; then
    check "zipalign -c -P 16 4 (stored libraries on 16 KB boundaries)" "$ZIPALIGN" -c -P 16 4 "$APK"
else
    note "zipalign is not installed here, so the zip alignment is not checked"
fi

# ---- package, SDK levels, permissions --------------------------------------------------
badging="$("$BT/aapt2" dump badging "$APK")"
check "package name is org.hopforge.app" grep -q "^package: name='org.hopforge.app'" <<<"$badging"
check "minSdk 24" grep -q "^sdkVersion:'24'$" <<<"$badging"
check "targetSdk 36" grep -q "^targetSdkVersion:'36'$" <<<"$badging"
check "not debuggable" bash -c "! grep -q '^application-debuggable' <<<"\$1"" _ "$badging"
perms="$(grep '^uses-permission: name=' <<<"$badging" | sed "s/^uses-permission: name='\(.*\)'\$/\1/" | sort | tr '\n' ' ')"
expected="android.permission.FOREGROUND_SERVICE android.permission.FOREGROUND_SERVICE_SPECIAL_USE android.permission.INTERNET android.permission.POST_NOTIFICATIONS "
if [ "$perms" = "$expected" ]; then
    pass "permissions are exactly INTERNET, FOREGROUND_SERVICE, FOREGROUND_SERVICE_SPECIAL_USE, POST_NOTIFICATIONS"
else
    fail "unexpected permissions: $perms"
fi
for abi in $ABIS; do
    check "native-code includes $abi" grep -q "^native-code:.*'$abi'" <<<"$badging"
done

# ---- manifest flags and exported components ---------------------------------------------
tree="$("$BT/aapt2" dump xmltree --file AndroidManifest.xml "$APK")"
check "android:debuggable is not set to true" bash -c "! grep -q 'android:debuggable(0x0101000f)=(type 0x12)0xffffffff' <<<"\$1"" _ "$tree"
check "android:allowBackup=false" grep -q 'android:allowBackup(0x01010280)=(type 0x12)0x0$' <<<"$tree"
check "android:usesCleartextTraffic=false" grep -q 'android:usesCleartextTraffic(0x010104ec)=(type 0x12)0x0$' <<<"$tree"
check "android:extractNativeLibs=true" grep -q 'android:extractNativeLibs(0x010104ea)=(type 0x12)0xffffffff$' <<<"$tree"
exported="$(grep -c 'android:exported(0x01010010)=(type 0x12)0xffffffff$' <<<"$tree" || true)"
if [ "$exported" = 1 ]; then
    pass "only the launcher activity is exported"
else
    fail "$exported components are exported (expected 1)"
fi

# ---- contents: nothing but code and resources that came from this repository -------------
listing="$(unzip -Z1 "$APK")"
unexpected="$(grep -v -E '^(AndroidManifest\.xml|classes\.dex|resources\.arsc|res/.+|assets/.+|lib/[a-z0-9_-]+/lib[a-z0-9-]+\.so)$' <<<"$listing" || true)"
if [ -z "$unexpected" ]; then
    pass "no unexpected files in the APK"
else
    fail "unexpected files in the APK:"
    echo "$unexpected" | sed 's/^/        /'
fi

# ---- ELF headers of every native executable ---------------------------------------------
system_libs=" libc.so libm.so libdl.so liblog.so libz.so libandroid.so libnetd_client.so "
unzip -q -o "$APK" 'lib/*' -d "$tmp"
for abi in $ABIS; do
    case "$abi" in
        arm64-v8a) machine="AArch64"; interp="/system/bin/linker64" ;;
        armeabi-v7a) machine="ARM"; interp="/system/bin/linker" ;;
        x86_64) machine="Advanced Micro Devices X86-64"; interp="/system/bin/linker64" ;;
        *) fail "unknown ABI $abi"; continue ;;
    esac
    for so in "$tmp/lib/$abi"/*.so; do
        name="$(basename "$so")"
        what="$abi/$name"
        header="$("$READELF" -h "$so")"
        check "$what is a $machine PIE ELF" bash -c "grep -q 'Machine:\s*$machine\$' <<<"\$1" && grep -qE 'Type:\s+(DYN|EXEC)' <<<"\$1"" _ "$header"
        program="$("$READELF" -lW "$so")"
        check "$what runs with $interp" grep -q "Requesting program interpreter: $interp\]" <<<"$program"
        aligned=yes
        while read -r typ _off _vaddr _paddr _filesz _memsz _flags align; do
            [ "$typ" = LOAD ] || continue
            if [ "$((16#${align}))" -lt 16384 ]; then
                aligned=no
                fail "$what has a LOAD segment aligned to $align (16 KB pages need 0x4000 or more)"
            fi
        done < <(grep '^ *LOAD ' <<<"$program" | awk '{$1=$1; print}')
        [ "$aligned" = yes ] && pass "$what LOAD segments are 16 KB aligned"
        dynamic="$("$READELF" -d "$so")"
        check "$what has no text relocations" bash -c "! grep -q '(TEXTREL)' <<<"\$1"" _ "$dynamic"
        check "$what has no RPATH/RUNPATH" bash -c "! grep -qE '\((RPATH|RUNPATH)\)' <<<"\$1"" _ "$dynamic"
        needed="$(grep NEEDED <<<"$dynamic" | sed 's/.*\[\(.*\)\]$/\1/' || true)"
        bad_libs=""
        for lib in $needed; do
            case "$system_libs" in *" $lib "*) ;; *) bad_libs="$bad_libs $lib" ;; esac
        done
        if [ -z "$bad_libs" ]; then
            pass "$what needs only Android system libraries ($(echo $needed | tr -s ' ' ',' | sed 's/^,//;s/,$//')"
        else
            fail "$what needs libraries that are not on every Android device:$bad_libs"
        fi
    done
done

if [ "$fails" -eq 0 ]; then
    echo "verify: all checks passed"
else
    echo "verify: $fails check(s) FAILED"
    exit 1
fi

#!/usr/bin/env python3
"""Inject a release signing config into the Tauri-generated Android Gradle script.

`tauri android init` regenerates `src-tauri/gen/android` on every CI run, and its
`app/build.gradle.kts` has no release `signingConfig`. This patches it to read the
keystore from `keystore.properties` (written separately from CI secrets), so
`tauri android build --apk` produces a signed, install-ready release APK.

Idempotent: running it twice is a no-op.
"""

import pathlib
import re
import sys

GRADLE = pathlib.Path("src-tauri/gen/android/app/build.gradle.kts")

SIGNING_BLOCK = """    signingConfigs {
        create("release") {
            val kpFile = rootProject.file("keystore.properties")
            val kp = Properties()
            if (kpFile.exists()) { kp.load(FileInputStream(kpFile)) }
            keyAlias = kp["keyAlias"] as String
            keyPassword = kp["password"] as String
            storeFile = file(kp["storeFile"] as String)
            storePassword = kp["password"] as String
        }
    }
"""


def main() -> int:
    if not GRADLE.exists():
        return f"not found: {GRADLE} (did `tauri android init` run?)"

    text = GRADLE.read_text()

    # Ensure the imports the signing block needs are present.
    prefix = ""
    if "import java.io.FileInputStream" not in text:
        prefix += "import java.io.FileInputStream\n"
    if "import java.util.Properties" not in text:
        prefix += "import java.util.Properties\n"
    text = prefix + text

    # Add the signingConfigs block at the top of the android { } block.
    if "signingConfigs {" not in text:
        text, n = re.subn(r"android\s*\{", lambda m: m.group(0) + "\n" + SIGNING_BLOCK, text, count=1)
        if n == 0:
            return "could not find the `android {` block to insert signingConfigs"

    # Attach the release signingConfig to the release build type.
    if 'signingConfig = signingConfigs.getByName("release")' not in text:
        text, n = re.subn(
            r'getByName\("release"\)\s*\{',
            lambda m: m.group(0) + '\n            signingConfig = signingConfigs.getByName("release")',
            text,
            count=1,
        )
        if n == 0:
            return "could not find the buildTypes `getByName(\"release\")` block"

    GRADLE.write_text(text)
    print(f"patched {GRADLE} for release signing")
    return 0


if __name__ == "__main__":
    sys.exit(main())

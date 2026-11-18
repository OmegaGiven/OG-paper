#!/usr/bin/env bash
# Builds the Play Store bundle (.aab) of OG Paper, signed with the upload key.
#
#   scripts/android-aab.sh VERSION_NAME VERSION_CODE OUT.aab
#
# cargo-apk builds the native library (it makes APKs only); this wraps it in a
# bundle: resources linked by aapt2 in proto format, packaging/android/
# AndroidManifest.xml, then bundletool and jarsigner.
# Needs: ANDROID_HOME (build-tools 36, platforms;android-36), ANDROID_NDK_ROOT,
# java, cargo-apk, and the upload key:
#   KEYSTORE            path of the .jks/.keystore
#   KEYSTORE_PASSWORD   its password (also the key's, unless KEY_PASSWORD)
#   KEY_ALIAS           optional: the key's alias (default: the keystore's only key)
# Optional: TARGET_SDK (36), BUNDLETOOL (a bundletool .jar; downloaded if unset).
set -euo pipefail
NAME=$1 CODE=$2 OUT=$3
TARGET_SDK=${TARGET_SDK:-36}
KEY_PASSWORD=${KEY_PASSWORD:-$KEYSTORE_PASSWORD}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
case "$OUT" in /*) ;; *) OUT=$PWD/$OUT ;; esac
BT_DIR=$(ls -d "$ANDROID_HOME"/build-tools/* | sort -V | tail -1)
AAPT2=$BT_DIR/aapt2
JAR=$ANDROID_HOME/platforms/android-$TARGET_SDK/android.jar
[ -f "$JAR" ] || { echo "!! missing $JAR (sdkmanager 'platforms;android-$TARGET_SDK')" >&2; exit 1; }
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

if [ -z "${BUNDLETOOL:-}" ]; then
  BUNDLETOOL=$WORK/bundletool.jar
  curl -fsSL -o "$BUNDLETOOL" https://github.com/google/bundletool/releases/download/1.18.3/bundletool-all-1.18.3.jar
fi
ALIAS=${KEY_ALIAS:-$(keytool -list -keystore "$KEYSTORE" -storepass "$KEYSTORE_PASSWORD" \
  | awk -F, '/PrivateKeyEntry/{print $1; exit}')}
[ -n "$ALIAS" ] || { echo "!! no key in $KEYSTORE" >&2; exit 1; }

echo "== native library (cargo-apk)"
cd "$ROOT"
CARGO_APK_RELEASE_KEYSTORE=$KEYSTORE CARGO_APK_RELEASE_KEYSTORE_PASSWORD=$KEYSTORE_PASSWORD \
  cargo apk build -p og-paper --release --lib 2>&1 | grep -vE '^\s+(Compiling|Downloaded|Downloading)' | tail -3
mkdir -p "$WORK/base"
unzip -q target/release/apk/og-paper.apk 'lib/*' -d "$WORK/base"
STRIP=$(ls "$ANDROID_NDK_ROOT"/toolchains/llvm/prebuilt/*/bin/llvm-strip | head -1)
find "$WORK/base/lib" -name '*.so' -exec "$STRIP" --strip-debug {} \;

echo "== resources and manifest (aapt2, proto)"
sed -e "s/__VERSION_CODE__/$CODE/" -e "s/__VERSION_NAME__/$NAME/" -e "s/__TARGET_SDK__/$TARGET_SDK/" \
  packaging/android/AndroidManifest.xml > "$WORK/AndroidManifest.xml"
"$AAPT2" compile --dir crates/og-paper/res -o "$WORK/res.zip"
"$AAPT2" link --proto-format -o "$WORK/linked.apk" -I "$JAR" \
  --manifest "$WORK/AndroidManifest.xml" --auto-add-overlay "$WORK/res.zip"
unzip -q "$WORK/linked.apk" -d "$WORK/linked"
mkdir -p "$WORK/base/manifest"
mv "$WORK/linked/AndroidManifest.xml" "$WORK/base/manifest/"
mv "$WORK/linked/resources.pb" "$WORK/base/"
mv "$WORK/linked/res" "$WORK/base/"
python3 -c 'import shutil, sys; shutil.make_archive(sys.argv[1], "zip", sys.argv[2])' "$WORK/base" "$WORK/base"

echo "== bundle (bundletool) and signature (jarsigner)"
rm -f "$OUT"
java -jar "$BUNDLETOOL" build-bundle --modules="$WORK/base.zip" --output="$OUT"
jarsigner -sigalg SHA256withRSA -digestalg SHA-256 -keystore "$KEYSTORE" \
  -storepass "$KEYSTORE_PASSWORD" -keypass "$KEY_PASSWORD" "$OUT" "$ALIAS" >/dev/null
jarsigner -verify "$OUT" >/dev/null
java -jar "$BUNDLETOOL" validate --bundle="$OUT" >/dev/null
echo "-> $OUT ($(du -h "$OUT" | cut -f1)) app.ogpaper $NAME ($CODE), target API $TARGET_SDK, key $ALIAS"

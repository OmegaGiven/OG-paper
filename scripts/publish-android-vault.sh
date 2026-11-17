#!/usr/bin/env bash
# Builds the OG Paper Android app on og-host2's forge runner and publishes it on our
# private mirror: og-host1 /var/lib/og-host/apps/og-vault/dist/og-paper/{og-paper.apk,release.json},
# which the vault serves at /api/v1/apps/paper/android(.apk) and lists on Home/Connect.
# Not the public mirror, not GitHub.
#   scripts/publish-android-vault.sh "What changed"
# Signed with og-host2's /cache/android/keys/og-paper.keystore (made on first use), so every
# build installs over the last one. An APK from GitHub's CI has another key: uninstall that
# one once before installing from the vault.
set -euo pipefail
HOST=root@192.168.0.137
DIST=/var/lib/og-host/apps/og-vault/dist/og-paper
VAULT=https://og-host1.tail6c6721.ts.net:8610
cd "$(dirname "$0")/.."
if [ -n "$(git status --porcelain -- crates Cargo.toml Cargo.lock)" ]; then
  echo "!! uncommitted changes: commit first so the version number means something" >&2
  exit 1
fi
# cargo-apk makes versionCode from the package version (each part < 256): it must only grow.
count="$(git rev-list --count HEAD)"
name="0.$((2 + count / 250)).$((count % 250))"
notes="${1:-$(git log -1 --format=%s)}"

echo "== building OG Paper $name on og-host2"
rm -rf target/release/apk
forge run --on og-host2 --no-fallback --pull target/release/apk "set -e
  . /cache/android/env.sh; unset ANDROID_SDK_ROOT
  export ANDROID_NDK_ROOT=/cache/android/sdk/ndk/27.2.12479018 PATH=/cache/cargo/bin:\$PATH
  ks=/cache/android/keys/og-paper.keystore pw=/cache/android/keys/og-paper.pass
  if [ ! -f \$ks ]; then
    head -c 24 /dev/urandom | base64 | tr -d '/+=' > \$pw; chmod 600 \$pw
    keytool -genkeypair -keystore \$ks -storepass \"\$(cat \$pw)\" -keypass \"\$(cat \$pw)\" -alias og-paper \
      -keyalg RSA -keysize 4096 -validity 20000 -dname 'CN=OG Paper,O=OmegaGiven' >/dev/null
    chmod 600 \$ks
  fi
  sed -i '0,/^version = .*/s//version = \"$name\"/' crates/og-paper/Cargo.toml
  export CARGO_APK_RELEASE_KEYSTORE=\$ks CARGO_APK_RELEASE_KEYSTORE_PASSWORD=\"\$(cat \$pw)\"
  cargo apk build -p og-paper --release --lib 2>&1 | grep -vE '^\s+(Compiling|Downloaded|Downloading)' | tail -5
  git checkout -- crates/og-paper/Cargo.toml 2>/dev/null || sed -i '0,/^version = .*/s//version = \"0.1.0\"/' crates/og-paper/Cargo.toml"
apk=target/release/apk/og-paper.apk
[ -f "$apk" ] || { echo "!! no APK came back" >&2; exit 1; }
code="$(python3 - "$name" <<'PY'
import sys
ma, mi, pa = (int(x) for x in sys.argv[1].split('.'))
# ndk-build VersionCode::to_code(1): 1 << 24 | major << 16 | minor << 8 | patch
# (checked with aapt2: 0.2.214 is 16777942)
print(1 << 24 | ma << 16 | mi << 8 | pa)
PY
)"
sha="$(sha256sum "$apk" | cut -d' ' -f1)"
size="$(stat -c %s "$apk")"
rel="$(python3 -c 'import json,sys,time; print(json.dumps({"versionCode": int(sys.argv[1]), "versionName": sys.argv[2], "sha256": sys.argv[3], "size": int(sys.argv[4]), "notes": sys.argv[5], "publishedAt": int(time.time()*1000)}))' "$code" "$name" "$sha" "$size" "$notes")"

echo "== publishing to og-host1"
ssh "$HOST" "mkdir -p $DIST && chmod 755 $DIST"
# APK first, then the manifest (renamed into place) so phones never see a half-published release.
ssh "$HOST" "cat > $DIST/og-paper.apk.new && chmod 644 $DIST/og-paper.apk.new && mv $DIST/og-paper.apk.new $DIST/og-paper.apk" < "$apk"
printf '%s\n' "$rel" | ssh "$HOST" "cat > $DIST/release.json.new && chmod 644 $DIST/release.json.new && mv $DIST/release.json.new $DIST/release.json"
ssh "$HOST" "sha256sum $DIST/og-paper.apk" | grep -q "$sha" || { echo "!! uploaded APK checksum differs" >&2; exit 1; }
curl -fsS --max-time 20 "$VAULT/api/v1/apps/paper/android" | grep -q "\"$name\"" && echo "-> vault serves $name" || echo "!! the vault doesn't list $name yet" >&2
echo "-> published OG Paper $name: $notes"
echo "   install: $VAULT/api/v1/apps/paper/android.apk"

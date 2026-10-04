#!/usr/bin/env bash
# Assemble OG Paper.app for iOS: assemble.sh BINARY APP_DIR iphoneos|iphonesimulator VERSION BUILD
# Puts in the executable, the Info.plist (with the build-machine keys Xcode
# would add) and the icon (an asset catalog). Signing is separate.
set -euo pipefail
BIN=$1 APP=$2 SDK=$3 VERSION=$4 BUILD=$5
HERE=$(cd "$(dirname "$0")" && pwd)
PLATFORM=$([ "$SDK" = iphoneos ] && echo iPhoneOS || echo iPhoneSimulator)
rm -rf "$APP" && mkdir -p "$APP"
cp "$BIN" "$APP/og-paper"
sed -e "s/__VERSION__/$VERSION/" -e "s/__BUILD__/$BUILD/" -e "s/__PLATFORM__/$PLATFORM/" \
  "$HERE/Info.plist.in" > "$APP/Info.plist"
# The icon: one 1024 px image; actool makes every size.
WORK=$(mktemp -d)
SET="$WORK/Assets.xcassets/AppIcon.appiconset"
mkdir -p "$SET"
cp "$HERE/../icon-1024.png" "$SET/icon-1024.png"
cat > "$SET/Contents.json" <<'JSON'
{"images":[{"idiom":"universal","platform":"ios","size":"1024x1024","filename":"icon-1024.png"}],"info":{"version":1,"author":"og-paper"}}
JSON
echo '{"info":{"version":1,"author":"og-paper"}}' > "$WORK/Assets.xcassets/Contents.json"
xcrun actool "$WORK/Assets.xcassets" --compile "$APP" --platform "$SDK" \
  --minimum-deployment-target 15.0 --app-icon AppIcon \
  --target-device iphone --target-device ipad \
  --output-partial-info-plist "$WORK/icon.plist" > /dev/null
/usr/libexec/PlistBuddy -c "Merge $WORK/icon.plist" "$APP/Info.plist"
# What Xcode records about the build (App Store validation reads these).
SDKVER=$(xcrun --sdk "$SDK" --show-sdk-version)
SDKBUILD=$(xcrun --sdk "$SDK" --show-sdk-build-version)
XCODE=$(xcodebuild -version | awk '/Xcode/{print $2}' | awk -F. '{printf "%d%d%d0", $1, $2, ($3==""?0:$3)}' | cut -c1-4)
XCBUILD=$(xcodebuild -version | awk '/Build version/{print $3}')
OSBUILD=$(sw_vers -buildVersion)
for kv in "DTPlatformName string $SDK" "DTPlatformVersion string $SDKVER" "DTPlatformBuild string $SDKBUILD" \
          "DTSDKName string $SDK$SDKVER" "DTSDKBuild string $SDKBUILD" "DTXcode string $XCODE" \
          "DTXcodeBuild string $XCBUILD" "DTCompiler string com.apple.compilers.llvm.clang.1_0" \
          "BuildMachineOSBuild string $OSBUILD"; do
  set -- $kv
  /usr/libexec/PlistBuddy -c "Add :$1 $2 $3" "$APP/Info.plist"
done
printf 'APPL????' > "$APP/PkgInfo"
plutil -lint "$APP/Info.plist"
ls "$APP"

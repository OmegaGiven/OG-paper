# Apple App Store packaging

- `icon-1024.*`: the store and iOS icon (full bleed; the system rounds
  the corners). `icon-mac-1024.*`: the Mac icon (a rounded plate in the
  standard margin). These, the web icons and the Android launcher icons
  all come from `packaging/make-icons.py`.
- `macos/OGPaper.entitlements`: the Mac App Store sandbox: files the user
  picks, network client (joining) and server (hosting). Canvases, fonts,
  plugins and settings live in the app's container.
- `macos/Info.plist.in`: the bundle's Info.plist (`__VERSION__` and
  `__BUILD__` are filled in by the workflow).

## Mac App Store builds

GitHub Actions > **Mac App Store** > Run workflow. It builds a universal
app, signs it, packages a signed `.pkg`, validates it with Apple and
uploads it to App Store Connect (TestFlight), unless "upload" is off. The
version must match the version being prepared in App Store Connect; the
build number is the run number, so it always grows.

Bundle ID `com.omegagiven.ogpaper`, team `FGFW7ZAJUC`. Repository secrets:

| Secret | What |
| --- | --- |
| `APPLE_DIST_CERTIFICATE`, `_PASSWORD` | Apple Distribution certificate (.p12, base64) and its password |
| `APPLE_DIST_SIGNING_IDENTITY` | `Apple Distribution: Nathan Johnson (FGFW7ZAJUC)` |
| `APPLE_INSTALLER_CERTIFICATE`, `_PASSWORD` | Mac Installer Distribution certificate (.p12, base64) and its password |
| `APPLE_INSTALLER_SIGNING_IDENTITY` | `3rd Party Mac Developer Installer: Nathan Johnson (FGFW7ZAJUC)` |
| `APPLE_MAC_PROVISIONING_PROFILE` | the Mac App Store profile (base64) |
| `APPLE_API_KEY`, `APPLE_API_KEY_ID`, `APPLE_API_ISSUER` | App Store Connect API key (.p8 contents), its ID and the issuer ID |

`.p12` files made with OpenSSL 3 need `openssl pkcs12 -export -legacy`, or
the Mac keychain refuses them.

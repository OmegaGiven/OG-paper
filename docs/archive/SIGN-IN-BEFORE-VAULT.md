# Sign-in before vault mode (archived 2026-11-16)

Superseded by [../VAULT.md](../VAULT.md) (og-paper task #792).

Until then OG Paper had one kind of account, OG Studio (ogstudio.org / omegagiven.com hosted
pages, [../HOSTED.md](../HOSTED.md)):

- **Web.** The account panel (My account) appeared only on a site with `<meta
  name="og-paper-api">`. Signed out it showed one **Sign in** button linking to the site's sign-in
  (`og-paper-signin`); the session was the site's cookie. Local state used fixed keys
  (`og-hosted-pages`, `og-hosted-canvases`, `og-hosted-sync`, `og-hosted-last`,
  `og-hosted-user`, `og-quota-warned`); OG Studio still uses them.
- **Native.** Branch `worktree-task-native-signin` (not yet on main then) added an OG Studio
  sign-in in Pages and a first-run card with **Sign in** / **Not now**, using the polling flow in
  [../NATIVE-SIGN-IN.md](../NATIVE-SIGN-IN.md). Android had no account (its build had no TLS and
  no INTERNET permission).
- The app's start was an inline `<script type="module">` in `web/app/index.html`.

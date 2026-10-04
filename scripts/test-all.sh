#!/usr/bin/env bash
# Everything that proves OG Paper works, by feature (see FEATURES.md):
# the Rust tests, the UI tests in a real browser (desktop and phone), and a
# check that every feature in FEATURES.md has a test that exists.
#
#   scripts/test-all.sh            build, run everything, report
#   scripts/test-all.sh --demo     also record every UI test as a demo video
#   scripts/test-all.sh --strict   fail if any feature has no test yet
set -uo pipefail
cd "$(dirname "$0")/.."
demo=0; strict=""
for a in "$@"; do
  case "$a" in --demo) demo=1 ;; --strict) strict="--strict" ;; esac
done
fail=0
step() { printf '\n\033[1m== %s\033[0m\n' "$1"; }

step "Rust tests"
cargo test --workspace -q 2>&1 | grep -E "^test result|FAILED|panicked" || true
cargo test --workspace -q >/dev/null 2>&1 || { echo "Rust tests failed"; fail=1; }

step "Build (desktop for the page server, web for the UI tests)"
cargo build -p og-paper -q || fail=1
./scripts/build-web.sh >/dev/null || fail=1

step "UI tests"
node tests/ui/run.mjs || fail=1

if [ "$demo" = 1 ]; then
  step "Demo videos"
  node tests/ui/run.mjs --demo || fail=1
fi

step "Features covered"
node tests/check-features.mjs $strict || fail=1

[ "$fail" = 0 ] && printf '\n\033[32mAll good.\033[0m\n' || printf '\n\033[31mSomething failed (see above).\033[0m\n'
exit $fail

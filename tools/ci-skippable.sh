#!/bin/sh
# Reads the names of the files a change touches, one per line, and prints build=false when none of them can
# change what a CI build or test does (the build jobs are then skipped), else build=true. No names: build=true.
#
# Can not matter to the builds: Markdown anywhere, notes/, .trail/ (the repository's own index) and the screenshots
# in docs/images/. docs/'s schemas (.json) are read by tests, so they build. ui/ is not in the list: a change there
# runs the ui-rust job (--ui) and skips the C++ builds only if it touches nothing else.
#   tools/ci-skippable.sh            names on stdin: build=true|false (the C++ builds)
#   tools/ci-skippable.sh --ui       names on stdin: ui=true|false (the ui-rust job: ui/, what ui/build.rs embeds
#                                    from hosts/standalone/profiles and themes, and .github/, the workflow itself)
#   tools/ci-skippable.sh --self-test
set -eu

decide() {
  awk 'BEGIN { n = 0; build = 0 }
    NF == 0 { next }
    { n++ }
    $0 ~ /\.md$/ || $0 ~ /^notes\// || $0 ~ /^\.trail\// || $0 ~ /^docs\/images\// { next }
    { build = 1 }
    END { print (n == 0 || build) ? "build=true" : "build=false" }'
}

decide_ui() {
  awk 'BEGIN { n = 0; ui = 0 }
    NF == 0 { next }
    { n++ }
    $0 ~ /^ui\// || $0 ~ /^hosts\/standalone\/(profiles|themes)\// || $0 ~ /^\.github\// { ui = 1 }
    END { print (n == 0 || ui) ? "ui=true" : "ui=false" }'
}

if [ "${1:-}" = "--ui" ]; then decide_ui; exit; fi

if [ "${1:-}" = "--self-test" ]; then
  checkui() { [ "$(printf '%s\n' "$2" | decide_ui)" = "ui=$1" ] || { echo "self-test failed (ui): $2 gave the wrong answer" >&2; exit 1; }; }
  checkui true 'ui/src/main.rs'
  checkui true 'hosts/standalone/themes/default/theme.json'
  checkui true '.github/workflows/ci.yml'
  checkui false 'core/control/src/setlist.cpp'
  checkui false 'docs/releasing.md'
  checkui true ''
  check() { [ "$(printf '%s\n' "$2" | decide)" = "build=$1" ] || { echo "self-test failed: $2 gave the wrong answer" >&2; exit 1; }; }
  check false 'docs/releasing.md'
  check false 'README.md
.trail/cache/x.md
notes/plans/a.txt
docs/images/desktop/home.png'
  check true 'ui/src/main.rs'
  check true 'docs/profile.schema.json'
  check true 'docs/releasing.md
core/control/src/setlist.cpp'
  check true '.github/workflows/ci.yml'
  check true 'tools/ci-skippable.sh'
  check true ''
  echo "ci-skippable: ok"
  exit 0
fi
decide

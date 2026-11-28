#!/bin/sh
# Reads the names of the files a change touches, one per line, and prints build=false when none of them can
# change what a CI build or test does (the build jobs are then skipped), else build=true. No names: build=true.
#
# Can not matter to the builds: Markdown anywhere, notes/, .trail/ (the repository's own index), the screenshots
# in docs/images/, and ui/ (the Rust UI: ci.yml builds it nowhere, only the release workflow does, and no C++ test
# reads it). docs/'s schemas (.json) are read by tests, so they build.
#   tools/ci-skippable.sh --self-test
set -eu

decide() {
  awk 'BEGIN { n = 0; build = 0 }
    NF == 0 { next }
    { n++ }
    $0 ~ /\.md$/ || $0 ~ /^notes\// || $0 ~ /^\.trail\// || $0 ~ /^docs\/images\// || $0 ~ /^ui\// { next }
    { build = 1 }
    END { print (n == 0 || build) ? "build=true" : "build=false" }'
}

if [ "${1:-}" = "--self-test" ]; then
  check() { [ "$(printf '%s\n' "$2" | decide)" = "build=$1" ] || { echo "self-test failed: $2 gave the wrong answer" >&2; exit 1; }; }
  check false 'docs/releasing.md'
  check false 'README.md
.trail/cache/x.md
notes/plans/a.txt
ui/src/main.rs
docs/images/desktop/home.png'
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

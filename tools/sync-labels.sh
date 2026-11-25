#!/bin/sh
# Create or update the labels of .github/labels.yml in each repository (never deletes labels).
# Usage: tools/sync-labels.sh [owner/repo ...]   (default: the list below; needs gh with repo scope)
set -eu
cd "$(dirname "$0")/.."
repos="$*"
[ -n "$repos" ] || repos="oscairn/oscairn oscairn/packages oscairn/sampler oscairn/va oscairn/fm oscairn/oscairn-clap oscairn/oscairn.org oscairn/samplepack oscairn/arpeggiator oscairn/discussions oscairn/RolandRevEng oscairn/YamahaMODXRevEng oscairn/private oscairn/performance-converter oscairn/va-private oscairn/sampler-private oscairn/measurement"
for repo in $repos; do
  sed -n 's/^- name: "\(.*\)"$/\1/p;s/^  color: "\(.*\)"$/\1/p;s/^  description: "\(.*\)"$/\1/p' .github/labels.yml |
  while read -r name && read -r color && read -r desc; do
    # GitHub answers an occasional 502: try again once before failing.
    gh label create "$name" --repo "$repo" --color "$color" --description "$desc" --force ||
      { sleep 10; gh label create "$name" --repo "$repo" --color "$color" --description "$desc" --force; }
  done
done

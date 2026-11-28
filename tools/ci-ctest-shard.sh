#!/bin/sh
# Run one slice of a build's tests, and first check that the slices are exactly the whole list.
# usage: tools/ci-ctest-shard.sh BUILD_DIR SHARDS SHARD EXCLUDE_REGEX [COSTS_FILE [INCLUDE_REGEX]]   (SHARD counts from 0)
# EXCLUDE_REGEX may be empty. The tests are dealt out by cost, longest first, each to the shard with the least
# so far (COSTS_FILE: lines "test-name seconds", tools/ci-costs/; a test it does not name counts as 1 s; without
# the file every test counts 1 s). The costs decide only the balance, never which tests run. A shard runs its
# tests by exact name (ctest -R), which also adds the fixture setup/cleanup tests they need, so a few cheap ones
# run in several shards: they are not counted as a shard's own. The check lists every shard's tests with
# ctest -N and fails if a test of the full list is in no shard or in two.
set -eu
dir=$1 shards=$2 shard=$3 exclude=$4 costs=${5:-/dev/null} include=${6:-}

# Prints the names the exclude and include filters select, one per line.
list() {
  ctest --test-dir "$dir" -N ${exclude:+-E "$exclude"} ${include:+-R "$include"} |
    sed -n 's/^ *Test *#[0-9]*: \(.*\)$/\1/p'
}
# the shard's own regex: its names, exactly
names_regex() { # $1: shard
  awk -v k="$1" '$1 == k { print $2 }' "$dir/shards-plan.txt" | sed 's/[][\.*^$+?(){}|]/\\&/g' | paste -sd'|' - | sed 's/^/^(/; s/$/)$/'
}

list > "$dir/shards-listed.txt"
[ -s "$dir/shards-listed.txt" ] || { echo "no tests listed" >&2; exit 1; }
sort "$dir/shards-listed.txt" > "$dir/shards-all.txt"

# every test's shard: "shard name"
awk 'NR == FNR { if ($0 !~ /^#/ && NF >= 2) cost[$1] = $2; next }
  { print (($1 in cost) ? cost[$1] : 1), $1 }' "$costs" "$dir/shards-listed.txt" |
  sort -k1,1nr -k2,2 |
  awk -v n="$shards" '{ best = 0; for (k = 1; k < n; k++) if (load[k] < load[best]) best = k
                        load[best] += $1; print best, $2 }' > "$dir/shards-plan.txt"

: > "$dir/shards-own.txt"
k=0
while [ "$k" -lt "$shards" ]; do
  re=$(names_regex "$k")
  [ -n "$re" ] && [ "$re" != "^()\$" ] || { echo "shard $k has no tests" >&2; exit 1; }
  printf '%s\n' "$re" > "$dir/shards-$k.re"
  ctest --test-dir "$dir" -N -R "$re" | sed -n 's/^ *Test *#[0-9]*: \(.*\)$/\1/p' > "$dir/shards-sel.txt"
  awk -v k="$k" 'NR == FNR { if ($1 == k) own[$2] = 1; next } ($1 in own) { print $1 }' "$dir/shards-plan.txt" "$dir/shards-sel.txt" >> "$dir/shards-own.txt"
  # what ctest selected beyond the shard's own tests are fixture setup/cleanup tests, a few
  extra=$(( $(wc -l < "$dir/shards-sel.txt") - $(awk -v k="$k" '$1 == k' "$dir/shards-plan.txt" | wc -l) ))
  [ "$extra" -le 10 ] || { echo "shard $k: ctest selected $extra tests beyond its own" >&2; exit 1; }
  k=$((k + 1))
done
sort "$dir/shards-own.txt" -o "$dir/shards-own.txt"
if ! diff "$dir/shards-all.txt" "$dir/shards-own.txt"; then
  echo "the $shards shards are not exactly the full test list (< in no shard, > in two or more)" >&2
  exit 1
fi
echo "shard check: $(wc -l < "$dir/shards-all.txt" | tr -d ' ') tests, each in exactly one of $shards shards"
echo "shard $shard: $(awk -v k="$shard" '$1 == k' "$dir/shards-plan.txt" | wc -l | tr -d ' ') of its own tests"

exec ctest --test-dir "$dir" --output-on-failure -R "$(cat "$dir/shards-$shard.re")"

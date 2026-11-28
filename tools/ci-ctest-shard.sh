#!/bin/sh
# Run one slice of a build's tests, and first check that the slices are exactly the whole list.
# usage: tools/ci-ctest-shard.sh BUILD_DIR SHARDS SHARD EXCLUDE_REGEX   (SHARD counts from 0)
# The slices are ctest's -I stride over the test numbers: shard k runs the tests numbered k+1, k+1+SHARDS, ...
# (ctest also adds the fixture setup/cleanup tests a slice's tests need, so a few cheap ones run in several
# shards: they are not counted as a shard's own). The check lists every shard's own tests with ctest -N and
# fails if a test of the full list is in no shard or in two.
set -eu
dir=$1 shards=$2 shard=$3 exclude=$4

ctest --test-dir "$dir" -N -E "$exclude" | sed -n 's/^ *Test *#[0-9]*: //p' | sort > "$dir/shards-all.txt"
[ -s "$dir/shards-all.txt" ] || { echo "no tests listed" >&2; exit 1; }
: > "$dir/shards-own.txt"
k=0
while [ "$k" -lt "$shards" ]; do
  ctest --test-dir "$dir" -N -E "$exclude" -I "$((k + 1)),,$shards" |
    sed -n 's/^ *Test *#\([0-9]*\): \(.*\)$/\1 \2/p' |
    awk -v s="$shards" -v k="$k" '($1 - 1) % s == k { print $2 }' >> "$dir/shards-own.txt"
  k=$((k + 1))
done
sort "$dir/shards-own.txt" -o "$dir/shards-own.txt"
if ! diff "$dir/shards-all.txt" "$dir/shards-own.txt"; then
  echo "the $shards shards are not exactly the full test list (< in no shard, > in two or more)" >&2
  exit 1
fi
echo "shard check: $(wc -l < "$dir/shards-all.txt" | tr -d ' ') tests, each in exactly one of $shards shards"

exec ctest --test-dir "$dir" --output-on-failure -E "$exclude" -I "$((shard + 1)),,$shards"

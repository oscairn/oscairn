#!/bin/sh
# Fails, printing each file:line, when anything under core/ uses JUCE: an #include naming juce
# or juce:: in a source file, or juce outside a comment in a CMake file. JUCE belongs in hosts/.
set -eu

cd "$(dirname "$0")/.."

sources=$(find core -type f \( -name '*.h' -o -name '*.hpp' -o -name '*.c' -o -name '*.cpp' -o -name '*.mm' -o -name '*.m' \) \
	-exec grep -nHiE -e '^[[:space:]]*#[[:space:]]*include[[:space:]]*[<"][^>"]*juce' -e 'juce::' {} + || true)
cmake=$(find core -type f \( -name CMakeLists.txt -o -name '*.cmake' \) -exec grep -nHi juce {} + \
	| grep -vE '^[^:]*:[0-9]+:[[:space:]]*#' || true)

found=$(printf '%s\n%s\n' "$sources" "$cmake" | sed '/^$/d')
if [ -n "$found" ]; then
	printf '%s\n' "$found"
	echo "core/ must stay free of JUCE; JUCE belongs in hosts/." >&2
	exit 1
fi

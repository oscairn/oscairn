# Sourced by tools/release-macos.sh and tools/release-linux.sh, from the repository's root:
# copy_licenses DIR JUCE_SOURCE puts Oscairn's license and the notices of what is built into the
# app (CREDITS.md says what and where) in DIR.
copy_licenses() {
	licenses=$1 juce=$2
	cp LICENSE "$licenses/Oscairn-AGPL-3.0.txt"
	cp CREDITS.md "$licenses/CREDITS.md"
	cp core/clap-host/third_party/clap/LICENSE "$licenses/CLAP-MIT.txt"
	cp core/clap-ext/LICENSE "$licenses/oscairn-clap-MIT.txt"
	cp core/control/third_party/miniz/LICENSE "$licenses/miniz-MIT.txt"
	cp core/control/third_party/monocypher/LICENCE.md "$licenses/Monocypher-BSD-2-Clause-or-CC0.md"
	cp hosts/standalone/icons/LICENSE "$licenses/Lucide-ISC.txt"
	cp hosts/standalone/themes/oscairn-dark/fonts/OFL.txt "$licenses/Inter-OFL-1.1.txt"
	cp "$juce/LICENSE.md" "$licenses/JUCE.md"
	cp "$juce/JUCE.spdx.json" "$licenses/JUCE.spdx.json"
	# What JUCE's modules the app links carry from others (JUCE.spdx.json lists all of them).
	mkdir "$licenses/JUCE"
	while read -r name path; do cp "$juce/modules/$path" "$licenses/JUCE/$name.txt"; done << EOF
zlib juce_core/zip/zlib/LICENSE
HarfBuzz juce_graphics/fonts/harfbuzz/COPYING
SheenBidi-Apache-2.0 juce_graphics/unicode/sheenbidi/LICENSE
LunaSVG juce_graphics/drawables/lunasvg/LICENSE
PlutoVG juce_graphics/drawables/lunasvg/plutovg/LICENSE
libpng juce_graphics/image_formats/pnglib/LICENSE
libwebp juce_graphics/image_formats/libwebp/COPYING
FLAC juce_audio_formats/codecs/flac/Flac Licence.txt
libogg juce_audio_formats/codecs/ogg/COPYING
libvorbis juce_audio_formats/codecs/vorbis/COPYING
Opus juce_audio_formats/codecs/opus/opus/COPYING
opusfile juce_audio_formats/codecs/opus/opusfile/COPYING
libopusenc juce_audio_formats/codecs/opus/libopusenc/COPYING
VST3-SDK juce_audio_processors_headless/format_types/VST3_SDK/LICENSE.txt
EOF
	echo "This software is based in part on the work of the Independent JPEG Group." > "$licenses/JUCE/libjpeg.txt"
}

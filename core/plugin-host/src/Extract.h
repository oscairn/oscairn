// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
#include <filesystem>
#include <string>

namespace oscairn
{
    // Oscairn's metadata extractors (RFC 0023 step 4), run in the scan helper as
    // `oscairn-plugin-scan --extract <extractor> <file>`, one untrusted file per process: what the
    // file holds as one JSON object, {"identity": <SHA-256 hex>, "fields": {...}}. "audio": WAV,
    // AIFF, FLAC and MP3; its identity is that of the audio alone, so retagging a file keeps it, and
    // its fields duration (seconds), rate (Hz), channels, title and artist, those the file has.
    // Throws std::runtime_error for an extractor it does not have or a file it cannot read.
    std::string extract(const std::string& extractor, const std::filesystem::path& file);
}

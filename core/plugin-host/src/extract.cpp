// SPDX-License-Identifier: AGPL-3.0-or-later
#include "Extract.h"
#include "oscairn/assets.h"
#include <algorithm>
#include <cctype>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <fstream>
#include <optional>
#include <stdexcept>

namespace oscairn
{
    namespace
    {
        namespace fs = std::filesystem;
        [[noreturn]] void fail(const std::string& why) { throw std::runtime_error(why); }

        // The file, read only where asked and never past its end.
        struct File
        {
            fs::path path;
            std::ifstream in;
            uint64_t size = 0;
            explicit File(const fs::path& p) : path(p), in(p, std::ios::binary)
            {
                std::error_code e;
                size = fs::file_size(p, e);
                if(!in || e) fail("cannot read the file");
            }
            std::string read(uint64_t at, uint64_t n)
            {
                if(at > size || n > size - at) fail("the file ends early");
                std::string out(size_t(n), '\0');
                in.clear(); in.seekg(std::streamoff(at));
                if(!in.read(out.data(), std::streamsize(n))) fail("cannot read the file");
                return out;
            }
        };
        uint32_t le(const std::string& s, size_t at, int n) { if(at + size_t(n) > s.size()) fail("the file ends early"); uint32_t v = 0; for(int i = n - 1; i >= 0; --i) v = v << 8 | uint8_t(s[at + size_t(i)]); return v; }
        uint32_t be(const std::string& s, size_t at, int n) { if(at + size_t(n) > s.size()) fail("the file ends early"); uint32_t v = 0; for(int i = 0; i < n; ++i) v = v << 8 | uint8_t(s[at + size_t(i)]); return v; }
        uint32_t syncsafe(const std::string& s, size_t at) { const auto v = be(s, at, 4); return (v & 0x7f) | (v >> 1 & 0x3f80) | (v >> 2 & 0x1fc000) | (v >> 3 & 0xfe00000); }

        void utf8(std::string& out, uint32_t c)
        {
            if(c < 0x80) out += char(c);
            else if(c < 0x800) { out += char(0xc0 | c >> 6); out += char(0x80 | (c & 63)); }
            else if(c < 0x10000) { out += char(0xe0 | c >> 12); out += char(0x80 | (c >> 6 & 63)); out += char(0x80 | (c & 63)); }
            else { out += char(0xf0 | c >> 18); out += char(0x80 | (c >> 12 & 63)); out += char(0x80 | (c >> 6 & 63)); out += char(0x80 | (c & 63)); }
        }
        bool validUtf8(const std::string& s)
        {
            for(size_t i = 0; i < s.size();)
            {
                const auto c = uint8_t(s[i]);
                const size_t n = c < 0x80 ? 0 : c >= 0xc2 && c <= 0xdf ? 1 : c >= 0xe0 && c <= 0xef ? 2 : c >= 0xf0 && c <= 0xf4 ? 3 : 9;
                if(n == 9 || i + n >= s.size() + (n ? 0 : 1)) return false;
                uint32_t code = n ? c & (0x3f >> n) : c;
                for(size_t k = 1; k <= n; ++k) { if((uint8_t(s[i + k]) & 0xc0) != 0x80) return false; code = code << 6 | (uint8_t(s[i + k]) & 63); }
                if((n == 2 && code < 0x800) || (n == 3 && (code < 0x10000 || code > 0x10ffff)) || (code >= 0xd800 && code <= 0xdfff)) return false;
                i += n + 1;
            }
            return true;
        }
        std::string latin1(const std::string& s) { std::string out; for(const char c : s) utf8(out, uint8_t(c)); return out; }
        std::string utf16(const std::string& s, bool bigEndian)
        {
            std::string out;
            for(size_t i = 0; i + 1 < s.size(); i += 2)
            {
                uint32_t c = bigEndian ? be(s, i, 2) : le(s, i, 2);
                if(c >= 0xd800 && c <= 0xdbff && i + 3 < s.size())
                {
                    const auto low = bigEndian ? be(s, i + 2, 2) : le(s, i + 2, 2);
                    if(low >= 0xdc00 && low <= 0xdfff) { c = 0x10000 + ((c - 0xd800) << 10) + (low - 0xdc00); i += 2; }
                }
                if(c == 0) break;
                if(c < 0xd800 || c > 0xdfff) utf8(out, c);
            }
            return out;
        }
        // Tag text as the library takes it: up to the first NUL, UTF-8 (else read as Latin-1), no
        // control characters, trimmed, at most 256 bytes.
        std::string text(std::string s)
        {
            s = s.substr(0, s.find('\0'));
            if(!validUtf8(s)) s = latin1(s);
            std::erase_if(s, [](char c) { return uint8_t(c) < 32 || c == 127; });
            while(!s.empty() && s.back() == ' ') s.pop_back();
            while(!s.empty() && s.front() == ' ') s.erase(0, 1);
            if(s.size() > 256) { s.resize(256); while(!s.empty() && (uint8_t(s.back()) & 0xc0) == 0x80) s.pop_back(); if(!s.empty() && uint8_t(s.back()) >= 0xc0) s.pop_back(); }
            return s;
        }
        std::string quote(const std::string& s)
        {
            std::string out = "\"";
            for(const char c : s) { if(c == '"' || c == '\\') out += '\\'; out += c; }
            return out + '"';
        }

        struct Audio { std::string identity, title, artist; double duration = -1; uint32_t rate = 0, channels = 0; };

        Audio wav(File& f)
        {
            Audio a;
            uint32_t align = 0;
            std::optional<std::pair<uint64_t, uint64_t>> data;
            bool format = false;
            for(uint64_t at = 12, n = 0; at + 8 <= f.size && n < 4096; ++n)
            {
                const auto c = f.read(at, 8);
                const auto id = c.substr(0, 4);
                const uint64_t size = le(c, 4, 4), body = at + 8;
                if(id == "data") data = {{body, std::min(size, f.size - body)}};
                else if(id == "fmt " && size >= 16) { const auto b = f.read(body, 16); a.channels = le(b, 2, 2); a.rate = le(b, 4, 4); align = le(b, 12, 2); format = true; }
                else if(id == "LIST" && size >= 4 && size <= (1u << 20) && size <= f.size - body)
                {
                    const auto b = f.read(body, size);
                    if(b.substr(0, 4) == "INFO")
                        for(size_t p = 4; p + 8 <= b.size();)
                        {
                            const auto key = b.substr(p, 4);
                            const auto len = le(b, p + 4, 4);
                            if(len > b.size() - p - 8) break;
                            if(key == "INAM") a.title = text(b.substr(p + 8, len));
                            if(key == "IART") a.artist = text(b.substr(p + 8, len));
                            p += 8 + len + (len & 1);
                        }
                }
                at = body + size + (size & 1);
            }
            if(!format || !data) fail("not a WAV file it can read");
            a.identity = assets::sha256File(f.path, data->first, data->second);
            if(align && a.rate) a.duration = double(data->second / align) / a.rate;
            return a;
        }

        Audio aiff(File& f)
        {
            Audio a;
            uint32_t frames = 0;
            double rate = 0;
            std::optional<std::pair<uint64_t, uint64_t>> data;
            bool common = false;
            for(uint64_t at = 12, n = 0; at + 8 <= f.size && n < 4096; ++n)
            {
                const auto c = f.read(at, 8);
                const auto id = c.substr(0, 4);
                const uint64_t size = be(c, 4, 4), body = at + 8, held = std::min(size, f.size - body);
                if(id == "COMM" && size >= 18)
                {
                    const auto b = f.read(body, 18);
                    a.channels = be(b, 0, 2); frames = be(b, 2, 4);
                    const int exponent = int(be(b, 8, 2) & 0x7fff);
                    const uint64_t mantissa = uint64_t(be(b, 10, 4)) << 32 | be(b, 14, 4);
                    rate = exponent ? std::ldexp(double(mantissa), exponent - 16383 - 63) : 0;
                    common = true;
                }
                else if(id == "SSND" && held >= 8)
                {
                    const uint64_t offset = be(f.read(body, 8), 0, 4);
                    if(offset <= held - 8) data = {{body + 8 + offset, held - 8 - offset}};
                }
                else if((id == "NAME" || id == "AUTH") && size <= 4096 && size <= f.size - body) (id == "NAME" ? a.title : a.artist) = text(f.read(body, size));
                at = body + size + (size & 1);
            }
            if(!common || !data) fail("not an AIFF file it can read");
            if(rate >= 1 && rate <= 10000000) { a.rate = uint32_t(std::lround(rate)); a.duration = frames / rate; }
            a.identity = assets::sha256File(f.path, data->first, data->second);
            return a;
        }

        // ID3v2 tags at the start (their text frames' titles and artists): where the audio starts.
        uint64_t id3(File& f, Audio& a)
        {
            uint64_t at = 0;
            for(int n = 0; n < 4 && at + 10 <= f.size; ++n)
            {
                const auto h = f.read(at, 10);
                if(h.substr(0, 3) != "ID3") break;
                const auto version = uint8_t(h[3]), flags = uint8_t(h[5]);
                const uint64_t size = syncsafe(h, 6), end = at + 10 + size + (flags & 0x10 ? 10 : 0);
                if(end > f.size) fail("an ID3 tag runs past the end of the file");
                // Frames of versions 3 and 4, when the whole tag is not unsynchronized.
                if((version == 3 || version == 4) && !(flags & 0x80) && size <= (4u << 20))
                {
                    const auto tag = f.read(at + 10, size);
                    size_t p = 0;
                    if(flags & 0x40) p = version == 3 ? 4 + size_t(be(tag, 0, 4)) : size_t(syncsafe(tag, 0));
                    while(p + 10 <= tag.size() && tag[p] != '\0')
                    {
                        const auto id = tag.substr(p, 4);
                        const size_t len = version == 4 ? syncsafe(tag, p + 4) : be(tag, p + 4, 4);
                        if(len > tag.size() - p - 10) break;
                        if((id == "TIT2" || id == "TPE1") && len >= 1)
                        {
                            const auto body = tag.substr(p + 11, len - 1);
                            const auto encoding = uint8_t(tag[p + 10]);
                            std::string value = encoding == 0 ? latin1(body.substr(0, body.find('\0')))
                                              : encoding == 1 && body.size() >= 2 ? utf16(body.substr(2), uint8_t(body[0]) == 0xfe)
                                              : encoding == 2 ? utf16(body, true) : body;
                            (id == "TIT2" ? a.title : a.artist) = text(value);
                        }
                        p += 10 + len;
                    }
                }
                at = end;
            }
            return at;
        }
        // Tags at the end, which retagging changes: an ID3v1 tag, and an APEv2 tag before it. Where the audio ends.
        uint64_t trailingTags(File& f, uint64_t from, Audio& a)
        {
            uint64_t end = f.size;
            if(end - from >= 128 && f.read(end - 128, 3) == "TAG")
            {
                const auto t = f.read(end - 128, 128);
                if(a.title.empty()) a.title = text(latin1(t.substr(3, 30).substr(0, t.substr(3, 30).find('\0'))));
                if(a.artist.empty()) a.artist = text(latin1(t.substr(33, 30).substr(0, t.substr(33, 30).find('\0'))));
                end -= 128;
            }
            if(end - from >= 32 && f.read(end - 32, 8) == "APETAGEX")
            {
                const auto footer = f.read(end - 32, 32);
                const uint64_t size = le(footer, 12, 4) + (le(footer, 20, 4) & 0x80000000u ? 32 : 0);
                if(size <= end - from) end -= size;
            }
            return end;
        }

        Audio flac(File& f, uint64_t at)
        {
            Audio a;
            if(f.read(at, 4) != "fLaC") fail("not a FLAC file");
            at += 4;
            bool info = false;
            for(int n = 0; n < 1024; ++n)
            {
                const auto h = f.read(at, 4);
                const bool last = uint8_t(h[0]) & 0x80;
                const int type = uint8_t(h[0]) & 0x7f;
                const uint64_t len = be(h, 1, 3);
                if(type == 0 && len >= 34)
                {
                    const auto b = f.read(at + 4, 34);
                    a.rate = be(b, 10, 3) >> 4;
                    a.channels = ((uint8_t(b[12]) >> 1) & 7) + 1;
                    const uint64_t samples = uint64_t(uint8_t(b[13]) & 0x0f) << 32 | be(b, 14, 4);
                    if(a.rate && samples) a.duration = double(samples) / a.rate;
                    info = true;
                }
                else if(type == 4 && len <= (1u << 20))
                {
                    const auto b = f.read(at + 4, len);
                    size_t p = 4 + size_t(le(b, 0, 4));
                    const auto count = le(b, p, 4);
                    p += 4;
                    for(uint32_t i = 0; i < count && i < 4096; ++i)
                    {
                        const size_t n = le(b, p, 4);
                        if(n > b.size() - p - 4) break;
                        const auto entry = b.substr(p + 4, n);
                        const auto eq = entry.find('=');
                        std::string key = entry.substr(0, eq);
                        for(auto& c : key) c = char(std::toupper(uint8_t(c)));
                        if(eq != std::string::npos && key == "TITLE") a.title = text(entry.substr(eq + 1));
                        if(eq != std::string::npos && key == "ARTIST") a.artist = text(entry.substr(eq + 1));
                        p += 4 + n;
                    }
                }
                at += 4 + len;
                if(last) break;
            }
            if(!info || at > f.size) fail("a FLAC file with no stream information, or cut short");
            Audio ignored;
            const auto end = trailingTags(f, at, ignored);
            a.identity = assets::sha256File(f.path, at, end - at);
            return a;
        }

        Audio mp3(File& f, uint64_t start, Audio a)
        {
            const auto end = trailingTags(f, start, a);
            if(end - start < 4) fail("not an MP3 file");
            const auto sync = f.read(start, 2);
            if(uint8_t(sync[0]) != 0xff || (uint8_t(sync[1]) & 0xe0) != 0xe0) fail("not an MP3 file");
            a.identity = assets::sha256File(f.path, start, end - start);
            return a;
        }

        std::string audio(const fs::path& path)
        {
            File f(path);
            if(f.size < 12) fail("not an audio file it can read");
            const auto head = f.read(0, 12);
            Audio a;
            if(head.substr(0, 4) == "RIFF" && head.substr(8, 4) == "WAVE") a = wav(f);
            else if(head.substr(0, 4) == "FORM" && (head.substr(8, 4) == "AIFF" || head.substr(8, 4) == "AIFC")) a = aiff(f);
            else
            {
                Audio tags;
                const auto start = id3(f, tags);
                if(start + 4 <= f.size && f.read(start, 4) == "fLaC") a = flac(f, start);
                else a = mp3(f, start, tags);
            }
            std::string fields;
            auto add = [&](const char* key, const std::string& value) { fields += std::string(fields.empty() ? "" : ",") + '"' + key + "\":" + value; };
            if(a.duration >= 0 && std::isfinite(a.duration)) { char d[32]; std::snprintf(d, sizeof d, "%.3f", a.duration); add("duration", d); }
            if(a.rate) add("rate", std::to_string(a.rate));
            if(a.channels) add("channels", std::to_string(a.channels));
            if(!a.title.empty()) add("title", quote(a.title));
            if(!a.artist.empty()) add("artist", quote(a.artist));
            return "{\"identity\":" + quote(a.identity) + ",\"fields\":{" + fields + "}}\n";
        }
    }

    std::string extract(const std::string& extractor, const fs::path& file)
    {
        if(extractor == "audio") return audio(file);
        fail("no extractor " + extractor);
    }
}

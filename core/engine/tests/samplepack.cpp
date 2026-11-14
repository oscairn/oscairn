// SPDX-License-Identifier: AGPL-3.0-or-later
// The pinned samplepack library links, and decodes one of its own test vectors to the PCM beside it.
#include <samplepack.h>
#include <cstring>
#include <fstream>
#include <iostream>
#include <iterator>
#include <string>
#include <vector>

static std::string read(const std::string& file)
{
    std::ifstream in(file, std::ios::binary);
    return {std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>()};
}

int main(int argc, char** argv)
{
    if(argc != 2) { std::cerr << "usage: oscairn-samplepack-test <samplepack test vector without extension>\n"; return 2; }
    const auto flac = read(std::string(argv[1]) + ".flac"), pcm = read(std::string(argv[1]) + ".pcm");
    const auto* data = reinterpret_cast<const uint8_t*>(flac.data());
    uint64_t at = 0;
    if(flac.empty() || pcm.empty() || sp_flac_first_frame(data, flac.size(), &at) != SP_OK) { std::cerr << "cannot read the test vector\n"; return 1; }
    static sp_flac_scratch scratch{};
    std::vector<int16_t> out, block(SP_FLAC_MAX_BLOCK);
    while(at < flac.size())
    {
        sp_flac_frame_info info{};
        if(const auto s = sp_flac_decode_frame(data + at, flac.size() - at, SP_FLAC_CHECK_CRC, &scratch, block.data(), nullptr, block.size(), &info); s != SP_OK || info.channels != 1)
        { std::cerr << "frame at " << at << ": status " << s << "\n"; return 1; }
        out.insert(out.end(), block.begin(), block.begin() + info.samples);
        at += info.consumed;
    }
    std::vector<int16_t> expected(pcm.size() / 2);
    for(size_t i = 0; i < expected.size(); ++i) expected[i] = int16_t(uint8_t(pcm[2 * i]) | uint8_t(pcm[2 * i + 1]) << 8);
    if(out != expected) { std::cerr << "decoded " << out.size() << " samples that differ from the " << expected.size() << " of the PCM\n"; return 1; }
    std::cout << "decoded " << out.size() << " samples as expected\n";
    return 0;
}

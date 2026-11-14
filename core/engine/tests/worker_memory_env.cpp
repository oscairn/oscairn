// SPDX-License-Identifier: AGPL-3.0-or-later
// oscairn-worker-memory-env-test VALUE EXPECTED_MB: with OSCAIRN_WORKER_MEMORY_MB=VALUE, the placement
// policy's worker memory limit is EXPECTED_MB megabytes (a value that is not a whole number leaves the default).
#include <oscairn/plugin_host.h>
#include <cstdio>
#include <cstdlib>

int main(int argc, char** argv)
{
    if(argc != 3) return 2;
    setenv("OSCAIRN_WORKER_MEMORY_MB", argv[1], 1);
    const auto limit = oscairn::placementPolicy().memoryLimit, expected = uint64_t(std::strtoull(argv[2], nullptr, 10)) << 20;
    if(limit != expected) { std::fprintf(stderr, "FAIL: OSCAIRN_WORKER_MEMORY_MB=%s gave %llu bytes, not %llu\n", argv[1], (unsigned long long)limit, (unsigned long long)expected); return 1; }
    return 0;
}

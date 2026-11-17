// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The one meaning of "relaxed" for the tests that read the clock (docs/developer-guide.md, Timing tests): a shared CI
// runner (CI is set, read when the test runs) or a sanitizer build slows and stalls what is timed. Only there may a
// timed scenario be measured again, or a claim be checked in the one direction a slow machine cannot fake. Off CI every
// assertion is exact.
#pragma once
#include <cstdlib>

namespace oscairn::test
{
    inline bool relaxed()
    {
#if defined(__SANITIZE_THREAD__) || defined(__SANITIZE_ADDRESS__)
        return true;
#elif defined(__has_feature)
#if __has_feature(thread_sanitizer) || __has_feature(address_sanitizer)
        return true;
#endif
#endif
        const char* ci = std::getenv("CI");
        return ci && *ci;
    }
}

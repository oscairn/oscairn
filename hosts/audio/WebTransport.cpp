// SPDX-License-Identifier: AGPL-3.0-or-later
// Package repositories on the web (RFC 0031): on macOS through JUCE's URL streams, so the
// system's NSURLSession; on Linux through libcurl directly, which verifies certificates against
// the system's CA bundle. Every byte fetched is checked by its hash and signature in the core.
//
// With the time unknown (RFC 0035), a certificate's dates are checked against the time floor
// only: one that expired before the floor is refused, one not yet valid at the floor is
// accepted, and the chain and host name are checked as always. Done for libcurl built with
// Mbed TLS, the device's; other TLS libraries keep their own date checks.
#include "WebTransport.h"
#include "oscairn/library.h"
#include "oscairn/repository.h"
#include <stdexcept>
#if defined(__linux__)
#include <algorithm>
#include <cstring>
#include <ctime>
#include <memory>
#include <curl/curl.h>
#if __has_include(<mbedtls/ssl.h>)
#define MBEDTLS_ALLOW_PRIVATE_ACCESS // the verify callback libcurl set, to call after ours
#include <mbedtls/ssl.h>
#include <mbedtls/x509_crt.h>
#ifndef MBEDTLS_PRIVATE
#define MBEDTLS_PRIVATE(member) member // Mbed TLS 2
#endif
#define OSCAIRN_FLOOR_DATES 1
#endif
#else
#include <juce_core/juce_core.h>
#endif

namespace oscairn::host
{
#if defined(__linux__)
    namespace
    {
        using Sink = std::function<bool(int, const char*, size_t)>;
        struct Transfer
        {
            CURL* curl;
            const Sink* sink;
            bool stopped = false;
#ifdef OSCAIRN_FLOOR_DATES
            int (*verify)(void*, mbedtls_x509_crt*, int, uint32_t*) = nullptr; // libcurl's
            void* verifying = nullptr;
            std::tm floor{};
#endif
        };

        int status(CURL* curl) { long code = 0; curl_easy_getinfo(curl, CURLINFO_RESPONSE_CODE, &code); return int(code); }

        size_t write(char* data, size_t, size_t n, void* user)
        {
            auto& t = *static_cast<Transfer*>(user);
            if((*t.sink)(status(t.curl), data, n)) return n;
            t.stopped = true;
            return 0; // ends the transfer
        }

#ifdef OSCAIRN_FLOOR_DATES
        bool before(const mbedtls_x509_time& t, const std::tm& floor)
        {
            const int a[]{t.year, t.mon, t.day, t.hour, t.min, t.sec};
            const int b[]{floor.tm_year + 1900, floor.tm_mon + 1, floor.tm_mday, floor.tm_hour, floor.tm_min, floor.tm_sec};
            return std::lexicographical_compare(a, a + 6, b, b + 6);
        }

        // Each certificate of the chain, after Mbed TLS has checked it against the clock.
        int floorDates(void* user, mbedtls_x509_crt* certificate, int depth, uint32_t* flags)
        {
            auto& t = *static_cast<Transfer*>(user);
            *flags &= ~uint32_t(MBEDTLS_X509_BADCERT_FUTURE | MBEDTLS_X509_BADCERT_EXPIRED);
            if(before(certificate->valid_to, t.floor)) *flags |= MBEDTLS_X509_BADCERT_EXPIRED;
            return t.verify ? t.verify(t.verifying, certificate, depth, flags) : 0;
        }

        CURLcode configure(CURL*, void* config, void* user)
        {
            auto& t = *static_cast<Transfer*>(user);
            auto& c = *static_cast<mbedtls_ssl_config*>(config);
            t.verify = c.MBEDTLS_PRIVATE(f_vrfy);
            t.verifying = c.MBEDTLS_PRIVATE(p_vrfy);
            c.MBEDTLS_PRIVATE(f_vrfy) = floorDates;
            c.MBEDTLS_PRIVATE(p_vrfy) = &t;
            return CURLE_OK;
        }
#endif
    }

    void useWebTransport()
    {
        static const auto initialized = curl_global_init(CURL_GLOBAL_DEFAULT);
        (void)initialized;
        repository::setTransport([](const std::string& url, uint64_t from, const Sink& sink) {
            const std::unique_ptr<CURL, void (*)(CURL*)> curl(curl_easy_init(), curl_easy_cleanup);
            if(!curl) throw std::runtime_error("cannot reach " + url + ": libcurl did not start");
            Transfer t{curl.get(), &sink};
            char error[CURL_ERROR_SIZE] = "";
            const auto range = std::to_string(from) + "-";
            auto* c = curl.get();
            curl_easy_setopt(c, CURLOPT_URL, url.c_str());
            curl_easy_setopt(c, CURLOPT_ERRORBUFFER, error);
            curl_easy_setopt(c, CURLOPT_NOSIGNAL, 1L);
            curl_easy_setopt(c, CURLOPT_FOLLOWLOCATION, 1L); // release downloads redirect to another host
            curl_easy_setopt(c, CURLOPT_MAXREDIRS, 5L);
            curl_easy_setopt(c, CURLOPT_CONNECTTIMEOUT, 15L);
            curl_easy_setopt(c, CURLOPT_LOW_SPEED_LIMIT, 100L); // as JUCE's streams did: under 100 bytes/s for 15 s ends it
            curl_easy_setopt(c, CURLOPT_LOW_SPEED_TIME, 15L);
            curl_easy_setopt(c, CURLOPT_USERAGENT, "Oscairn");
            curl_easy_setopt(c, CURLOPT_WRITEFUNCTION, write);
            curl_easy_setopt(c, CURLOPT_WRITEDATA, &t);
            if(from) curl_easy_setopt(c, CURLOPT_RANGE, range.c_str());
#ifdef OSCAIRN_FLOOR_DATES
            const auto* version = curl_version_info(CURLVERSION_NOW);
            if(!timeKnown() && version->ssl_version && std::strncmp(version->ssl_version, "mbedTLS", 7) == 0)
            {
                const auto floor = std::chrono::system_clock::to_time_t(timeFloor());
                gmtime_r(&floor, &t.floor);
                curl_easy_setopt(c, CURLOPT_SSL_CTX_FUNCTION, configure);
                curl_easy_setopt(c, CURLOPT_SSL_CTX_DATA, &t);
            }
#endif
            const auto result = curl_easy_perform(c);
            const int code = status(c);
            // Cut off midway, what arrived is kept and checked by its hash, as before.
            if(result != CURLE_OK && !t.stopped && code == 0) throw std::runtime_error("cannot reach " + url + ": " + (*error ? error : curl_easy_strerror(result)));
            return code;
        });
    }
#else
    void useWebTransport()
    {
        repository::setTransport([](const std::string& url, uint64_t from, const std::function<bool(int, const char*, size_t)>& sink) {
            int status = 0;
            auto options = juce::URL::InputStreamOptions(juce::URL::ParameterHandling::inAddress)
                               .withConnectionTimeoutMs(15000)
                               .withNumRedirectsToFollow(5) // release downloads redirect to another host
                               .withStatusCode(&status)
                               .withExtraHeaders(from ? "Range: bytes=" + juce::String(juce::int64(from)) + "-" : juce::String());
            const auto stream = juce::URL(juce::String::fromUTF8(url.c_str())).createInputStream(options);
            if(!stream) throw std::runtime_error("cannot reach " + url);
            char buffer[1 << 16];
            for(;;)
            {
                const auto n = stream->read(buffer, sizeof buffer);
                if(n <= 0 || !sink(status, buffer, size_t(n))) break;
            }
            return status;
        });
    }
#endif
}

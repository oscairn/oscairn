// SPDX-License-Identifier: AGPL-3.0-or-later
//
// oscairn-engine-timing SOCKET [--seconds N] [--no-reset]: the engine's audio timing over an
// interval, from its control socket (engine.timing and session.status in the control protocol).
// Resets the engine's timing window, waits N seconds (10 by default), and prints each measurement's
// percentiles and its steady histogram, with the deadline misses and underruns over the interval.
// --no-reset prints the window since the last reset instead, at once unless --seconds is given.
#include "JsonInput.h"
#include "oscairn/timing.h"

#include <cerrno>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <stdexcept>
#include <string>
#include <thread>
#include <vector>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

namespace
{
    using V = const json_value_s*;
    using Doc = std::unique_ptr<json_value_s, decltype(&std::free)>;

    V field(V v, const char* key)
    {
        if(V m = oscairn::detail::member(v, key)) return m;
        throw std::runtime_error(std::string("the engine's answer has no ") + key);
    }
    double number(V v)
    {
        if(!v || v->type != json_type_number) throw std::runtime_error("the engine's answer has a number missing");
        const auto* n = static_cast<const json_number_s*>(v->payload);
        return std::strtod(std::string(n->number, n->number_size).c_str(), nullptr);
    }
    std::string text(V v)
    {
        if(!v || v->type != json_type_string) return {};
        const auto* s = static_cast<const json_string_s*>(v->payload);
        return {s->string, s->string_size};
    }
    std::vector<V> array(V v)
    {
        std::vector<V> out;
        if(v && v->type == json_type_array) for(auto* e = static_cast<const json_array_s*>(v->payload)->start; e; e = e->next) out.push_back(e->value);
        return out;
    }

    struct Connection
    {
        int fd = -1;
        std::string pending;
        int id = 0;
        explicit Connection(const char* path)
        {
            sockaddr_un address{};
            address.sun_family = AF_UNIX;
            if(std::strlen(path) >= sizeof address.sun_path) throw std::runtime_error("socket path too long");
            std::strcpy(address.sun_path, path);
            fd = socket(AF_UNIX, SOCK_STREAM, 0);
            if(fd < 0 || connect(fd, reinterpret_cast<sockaddr*>(&address), sizeof address) != 0) throw std::runtime_error(std::string(path) + ": " + std::strerror(errno));
        }
        ~Connection() { if(fd >= 0) close(fd); }
        // One request's result; notifications (no id) are skipped.
        Doc call(const std::string& method, const std::string& params)
        {
            const auto line = "{\"jsonrpc\":\"2.0\",\"id\":" + std::to_string(++id) + ",\"method\":\"" + method + "\",\"params\":" + params + "}\n";
            for(size_t sent = 0; sent < line.size();)
            {
                const auto n = write(fd, line.data() + sent, line.size() - sent);
                if(n <= 0) throw std::runtime_error("the engine closed the connection");
                sent += size_t(n);
            }
            for(;;)
            {
                size_t end;
                while((end = pending.find('\n')) == std::string::npos)
                {
                    char buffer[65536];
                    const auto n = read(fd, buffer, sizeof buffer);
                    if(n <= 0) throw std::runtime_error("the engine closed the connection");
                    pending.append(buffer, size_t(n));
                }
                Doc doc(json_parse(pending.data(), end), &std::free);
                pending.erase(0, end + 1);
                if(!doc || !oscairn::detail::member(doc.get(), "id")) continue;
                if(V error = oscairn::detail::member(doc.get(), "error")) throw std::runtime_error(method + ": " + text(oscairn::detail::member(error, "message")));
                return doc;
            }
        }
    };

    oscairn::Histogram histogram(V v)
    {
        oscairn::Histogram h;
        size_t i = 0;
        for(V n : array(field(v, "buckets"))) if(i < h.buckets) h.counts[i++] = uint64_t(number(n));
        h.maxNs = int64_t(number(field(v, "max")) * 1e3);
        h.maxAt = int64_t(number(field(v, "maxAt")) * 1e9);
        return h;
    }

    struct Row { std::string name; oscairn::Histogram steady, startup; };

    void summary(const char* what, const std::string& name, const oscairn::Histogram& h)
    {
        std::printf("%-24s %-8s %8llu", name.c_str(), what, static_cast<unsigned long long>(h.count()));
        if(!h.count()) { std::printf("\n"); return; }
        std::printf(" %7.0f %7.0f %7.0f %9.1f %8.2f\n", h.percentileMicroseconds(.5), h.percentileMicroseconds(.99), h.percentileMicroseconds(.999), h.maxNs / 1e3, h.maxAt / 1e9);
    }
}

int main(int argc, char** argv)
{
    if(argc < 2 || argv[1][0] == '-')
    {
        std::fprintf(stderr, "usage: oscairn-engine-timing SOCKET [--seconds N] [--no-reset]\n");
        return 2;
    }
    double seconds = -1;
    bool reset = true;
    for(int i = 2; i < argc; ++i)
    {
        const std::string arg = argv[i];
        if(arg == "--seconds" && i + 1 < argc) seconds = std::atof(argv[++i]);
        else if(arg == "--no-reset") reset = false;
        else { std::fprintf(stderr, "unknown argument %s\n", arg.c_str()); return 2; }
    }
    if(seconds < 0) seconds = reset ? 10 : 0;
    try
    {
        Connection engine(argv[1]);
        auto counts = [&] {
            auto status = engine.call("session.status", "{}");
            V r = field(status.get(), "result");
            return std::pair{number(field(r, "deadlineMisses")), number(field(r, "audioUnderruns"))};
        };
        const auto before = counts();
        if(reset) engine.call("engine.timing", "{\"reset\":true}");
        std::this_thread::sleep_for(std::chrono::duration<double>(seconds));
        const auto timing = engine.call("engine.timing", "{}");
        const auto after = counts();
        V r = field(timing.get(), "result");

        std::vector<Row> rows;
        auto add = [&](const std::string& name, V m) { rows.push_back({name, histogram(field(m, "steady")), histogram(field(m, "startup"))}); };
        add("callback duration", field(r, "callback"));
        add("callback lateness", field(r, "lateness"));
        // Each worker's round trip, then its parts: until the engine came for the block (collect), until
        // the worker's audio thread ran (wake), and the worker's run.
        for(V w : array(field(r, "workers")))
        {
            const auto name = text(field(w, "name")) + " (" + std::to_string(int(number(field(w, "node")))) + ")";
            add(name + " round trip", field(w, "roundTrip"));
            for(const char* part : {"collect", "wake", "run"}) add(name + " " + part, field(w, part));
        }

        std::printf("engine timing over %.1f s (the window: %.1f s)%s; times in microseconds, a percentile as its bucket's upper bound\n",
                    seconds, number(field(field(r, "callback"), "seconds")), reset ? "" : ", since the last reset");
        std::printf("deadline misses %+.0f, audio underruns %+.0f over the interval (now %.0f and %.0f)\n",
                    after.first - before.first, after.second - before.second, after.first, after.second);
        // Each worker's blocks past its allowance in the window: played late, or silenced (heard as a dropout),
        // of those not handed over because the worker was busy; the panics that followed a block it did not
        // render (each ends the part's notes); its audio thread's page faults.
        for(V w : array(field(r, "workers")))
            std::printf("%s (%d): allowance %.0f, past it %.0f played late and %.0f silenced (%.0f not handed: busy), %.0f panics after a miss, page faults %.0f minor %.0f major\n",
                        text(field(w, "name")).c_str(), int(number(field(w, "node"))), number(field(w, "allowance")), number(field(w, "late")), number(field(w, "silent")),
                        number(field(w, "busy")), number(field(w, "missPanics")), number(field(w, "minorFaults")), number(field(w, "majorFaults")));
        std::printf("startup: the first %.0f blocks after a start (engine, device, Performance load, worker restart)\n\n", number(field(r, "warmupBlocks")));
        std::printf("%-24s %-8s %8s %7s %7s %7s %9s %8s\n", "measurement", "", "blocks", "p50", "p99", "p99.9", "max", "max at s");
        for(const auto& row : rows) { summary("steady", row.name, row.steady); if(row.startup.count()) summary("startup", "", row.startup); }

        std::printf("\nsteady histograms, blocks per bucket:\n%-14s", "bucket (us)");
        for(size_t i = 0; i < rows.size(); ++i) std::printf(" %10s", ("[" + std::to_string(i + 1) + "]").c_str());
        std::printf("\n");
        for(size_t b = 0; b < oscairn::Histogram::buckets; ++b)
        {
            bool any = false;
            for(const auto& row : rows) any = any || row.steady.counts[b];
            if(!any) continue;
            char label[32];
            if(b == 0) std::snprintf(label, sizeof label, "< 16");
            else if(b + 1 == oscairn::Histogram::buckets) std::snprintf(label, sizeof label, ">= %llu", static_cast<unsigned long long>(oscairn::Histogram::upperMicroseconds(b - 1)));
            else std::snprintf(label, sizeof label, "%llu-%llu", static_cast<unsigned long long>(oscairn::Histogram::upperMicroseconds(b - 1)), static_cast<unsigned long long>(oscairn::Histogram::upperMicroseconds(b)));
            std::printf("%-14s", label);
            for(const auto& row : rows) std::printf(" %10llu", static_cast<unsigned long long>(row.steady.counts[b]));
            std::printf("\n");
        }
        for(size_t i = 0; i < rows.size(); ++i) std::printf("[%zu] %s\n", i + 1, rows[i].name.c_str());
        return 0;
    }
    catch(const std::exception& e)
    {
        std::fprintf(stderr, "oscairn-engine-timing: %s\n", e.what());
        return 1;
    }
}

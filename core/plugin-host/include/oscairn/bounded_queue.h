// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
#include <array>
#include <atomic>
#include <cstddef>
#include <cstdint>

namespace oscairn {
// Bounded Vyukov queue: many producers, one consumer. Producers claim independent slots; the consumer (an audio thread)
// never spins on an unfinished producer. No allocation, locks or reclamation.
template<class T, size_t Capacity> class BoundedQueue {
    struct Slot { std::atomic<size_t> sequence{0}; T value{}; };
    std::array<Slot, Capacity> slots;
    alignas(64) std::atomic<size_t> write{0};
    alignas(64) size_t read = 0;
public:
    BoundedQueue() { for (size_t i = 0; i < Capacity; ++i) slots[i].sequence.store(i); }
    bool push(const T& value) noexcept {
        auto position = write.load(std::memory_order_relaxed);
        for (;;) {
            auto& slot = slots[position % Capacity];
            const auto seq = slot.sequence.load(std::memory_order_acquire);
            const auto difference = static_cast<intptr_t>(seq - position);
            if (difference == 0) {
                if (write.compare_exchange_weak(position, position + 1, std::memory_order_relaxed)) {
                    slot.value = value;
                    slot.sequence.store(position + 1, std::memory_order_release);
                    return true;
                }
            } else if (difference < 0) return false;
            else position = write.load(std::memory_order_relaxed);
        }
    }
    bool pop(T& value) noexcept {
        auto& slot = slots[read % Capacity];
        if (slot.sequence.load(std::memory_order_acquire) != read + 1) return false;
        value = slot.value;
        slot.sequence.store(read + Capacity, std::memory_order_release);
        ++read;
        return true;
    }
};
} // namespace oscairn

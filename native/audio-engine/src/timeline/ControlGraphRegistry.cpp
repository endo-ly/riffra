#include "ControlGraphRegistry.h"

#include <algorithm>

namespace riffra {

std::uint32_t TrackKeyRegistry::keyFor(const juce::String& trackId) {
    const auto [entry, inserted] = keys.try_emplace(trackId, nextKey);
    if (inserted) ++nextKey;
    return entry->second;
}

PreparedTimeline* ControlGraphRegistry::State::find(const std::uint64_t serial) const noexcept {
    for (const auto& graph : committed)
        if (graph->serial == serial) return graph.get();
    return nullptr;
}

ControlGraphRegistry::ControlGraphRegistry() noexcept : reclaimThread(std::this_thread::get_id()) {}

std::size_t ControlGraphRegistry::reclaim(Retired& retired) {
    if (std::this_thread::get_id() != reclaimThread) return 0;
    std::vector<std::unique_ptr<PreparedTimeline>> destroyed;
    {
        const std::lock_guard lock(mutex);
        retired.reclaim([this, &destroyed](PreparedTimeline* graph) {
            const auto owned =
                std::find_if(state.committed.begin(), state.committed.end(),
                             [graph](const auto& item) { return item.get() == graph; });
            jassert(owned != state.committed.end());
            destroyed.push_back(std::move(*owned));
            state.committed.erase(owned);
        });
    }
    return destroyed.size();
}

}  // namespace riffra

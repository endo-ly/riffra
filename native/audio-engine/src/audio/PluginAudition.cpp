#include "PluginAudition.h"

#include <array>
#include <chrono>
#include <thread>
#include <utility>

namespace riffra {

PluginAudition::Use::Use(PluginAudition& owner) noexcept : audition(owner) {
    audition.users.fetch_add(1, std::memory_order_seq_cst);
    current = audition.active.load(std::memory_order_seq_cst);
}

PluginAudition::Use::~Use() { audition.users.fetch_sub(1, std::memory_order_seq_cst); }

void PluginAudition::install(std::unique_ptr<PluginRack> rack, const double sampleRate,
                             const int blockSize) {
    jassert(owned == nullptr);
    auto next = std::make_unique<Installed>();
    next->instrument = rack->isInstrument();
    next->rack = std::move(rack);
    next->renderBuffer.setSize(2, blockSize);
    next->sampleRate = sampleRate;
    next->blockSize = blockSize;
    owned = std::move(next);
    active.store(owned.get(), std::memory_order_seq_cst);
}

std::unique_ptr<PluginRack> PluginAudition::uninstall() noexcept {
    if (owned == nullptr) return nullptr;
    owned->rack->allNotesOff();
    owned->stopping.store(true, std::memory_order_release);
    // The fade takes one callback. A device that stopped, or changed its
    // format, never runs it, so waiting is limited to a few callback periods.
    const auto callbackPeriod = std::chrono::duration<double>(owned->blockSize / owned->sampleRate);
    const auto deadline = std::chrono::steady_clock::now() + 4 * callbackPeriod;
    while (!owned->faded.load(std::memory_order_acquire) &&
           std::chrono::steady_clock::now() < deadline)
        std::this_thread::yield();
    active.store(nullptr, std::memory_order_seq_cst);
    while (users.load(std::memory_order_seq_cst) != 0) std::this_thread::yield();
    const auto removed = std::exchange(owned, nullptr);
    return std::move(removed->rack);
}

PluginRack* PluginAudition::installed() const noexcept {
    return owned != nullptr ? owned->rack.get() : nullptr;
}

bool PluginAudition::enqueueMidi(const juce::MidiMessage& message) noexcept {
    const Use use(*this);
    auto* current = use.get();
    if (current == nullptr || !current->instrument ||
        current->stopping.load(std::memory_order_acquire))
        return false;
    // A full queue drops the message here rather than sending it to a Track.
    (void)current->rack->enqueueMidi(message);
    return true;
}

bool PluginAudition::monitorsInput() noexcept {
    const Use use(*this);
    const auto* current = use.get();
    return current != nullptr && !current->instrument &&
           !current->faded.load(std::memory_order_acquire);
}

void PluginAudition::mix(const float* const input, float* const* outputChannels,
                         const int outputChannelCount, const int numSamples,
                         const double sampleRate) noexcept {
    const Use use(*this);
    auto* current = use.get();
    if (current == nullptr || current->faded.load(std::memory_order_acquire) ||
        sampleRate != current->sampleRate || numSamples <= 0 || numSamples > current->blockSize)
        return;
    auto& buffer = current->renderBuffer;
    std::array<float*, 2> render{buffer.getWritePointer(0), buffer.getWritePointer(1)};
    const std::array<const float*, 1> inputs{input};
    const auto inputCount = !current->instrument && input != nullptr ? 1 : 0;
    current->rack->process(inputs.data(), inputCount, render.data(), 2, numSamples);
    const auto stopping = current->stopping.load(std::memory_order_acquire);
    for (int channel = 0; channel < outputChannelCount; ++channel) {
        auto* output = outputChannels[channel];
        if (output == nullptr) continue;
        const auto* rendered = buffer.getReadPointer(juce::jmin(channel, 1));
        if (stopping) {
            for (int sample = 0; sample < numSamples; ++sample)
                output[sample] +=
                    rendered[sample] * (1.0f - static_cast<float>(sample + 1) / numSamples);
        } else {
            juce::FloatVectorOperations::add(output, rendered, numSamples);
        }
    }
    if (stopping) current->faded.store(true, std::memory_order_release);
}

}  // namespace riffra

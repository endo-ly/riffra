#include "RigEngine.h"

#include <algorithm>
#include <chrono>
#include <cmath>
#include <thread>
#include <utility>

namespace riffra {

RigEngine::Use::Use(RigEngine& owner) noexcept : engine(owner) {
    engine.users.fetch_add(1, std::memory_order_seq_cst);
    current = engine.active.load(std::memory_order_seq_cst);
}

RigEngine::Use::~Use() { engine.users.fetch_sub(1, std::memory_order_seq_cst); }

bool RigEngine::keepsPreparedChain(const RigProjectionSpec& projection, const double sampleRate,
                                   const int blockSize) const {
    return owned != nullptr && owned->projection.rigId == projection.rigId &&
           owned->sampleRate == sampleRate && owned->blockSize == blockSize &&
           sameEffectTopology(owned->projection.effects, projection.effects);
}

bool RigEngine::install(const RigProjectionSpec& projection, const double sampleRate,
                        const int blockSize, juce::String& error) {
    if (projection.rigId.isEmpty()) {
        error = "A rig projection requires a rig id.";
        return false;
    }
    if (!std::isfinite(projection.gainDb)) {
        error = "A rig projection requires a finite gain.";
        return false;
    }
    if (!(sampleRate > 0.0) || blockSize <= 0) {
        error = "A rig requires an active audio device.";
        return false;
    }
    std::vector<juce::String> deviceIds;
    deviceIds.reserve(projection.effects.size());
    for (const auto& effect : projection.effects) {
        if (effect.id.isEmpty() || effect.path.isEmpty()) {
            error = "A rig effect requires an id and path.";
            return false;
        }
        if (std::find(deviceIds.begin(), deviceIds.end(), effect.id) != deviceIds.end()) {
            error = "Rig effect ids must be unique.";
            return false;
        }
        deviceIds.push_back(effect.id);
    }

    const auto reuse = keepsPreparedChain(projection, sampleRate, blockSize);
    auto chain = reuse ? owned->chain : std::make_shared<PluginChain>();
    if (!reuse && !chain->load(projection.effects, {}, sampleRate, blockSize,
                               PluginProcessingMode::realtime, error, "rig"))
        return false;
    // Every projection is authoritative for the bypass of the devices it carries.
    // Plugin state is not applied to a reused chain: it only changes through the
    // device's editor, which edits the prepared instance directly, and a stored
    // snapshot can be older than the state the instance already holds.
    for (const auto& effect : projection.effects)
        chain->setBypassed(effect.id, effect.state.bypassed);

    auto next = std::make_unique<Installed>();
    next->projection = projection;
    next->chain = std::move(chain);
    next->sampleRate = sampleRate;
    next->blockSize = blockSize;
    next->renderBuffer.setSize(2, blockSize, false, true, false);

    auto previous = std::exchange(owned, std::move(next));
    active.store(owned.get(), std::memory_order_seq_cst);
    while (users.load(std::memory_order_seq_cst) != 0) std::this_thread::yield();
    (void)previous;
    // The previous instance is destroyed here, on the lifecycle thread.
    return true;
}

void RigEngine::uninstall() noexcept {
    if (owned == nullptr) return;
    owned->stopping.store(true, std::memory_order_release);
    if (owned->sampleRate > 0.0 && owned->blockSize > 0) {
        // The fade takes one callback. A device that stopped, or changed its
        // format, never runs it, so the wait is bounded.
        const auto deadline = std::chrono::steady_clock::now() + std::chrono::milliseconds(200);
        while (!owned->faded.load(std::memory_order_acquire) &&
               std::chrono::steady_clock::now() < deadline)
            std::this_thread::sleep_for(std::chrono::microseconds(100));
    }
    active.store(nullptr, std::memory_order_seq_cst);
    while (users.load(std::memory_order_seq_cst) != 0) std::this_thread::yield();
    owned.reset();
}

juce::String RigEngine::rigId() {
    const Use use(*this);
    const auto* current = use.get();
    return current != nullptr ? current->projection.rigId : juce::String{};
}

std::shared_ptr<PluginRack> RigEngine::findRack(const juce::String& deviceId) {
    const Use use(*this);
    const auto* current = use.get();
    if (current == nullptr) return {};
    auto chain = current->chain;
    auto* rack = chain->findDevice(deviceId);
    if (rack == nullptr) return {};
    // The device is owned by the prepared chain, so the handle keeps the chain
    // alive for as long as its editor, or its captured state, is used.
    return std::shared_ptr<PluginRack>(std::move(chain), rack);
}

std::uint64_t RigEngine::revision() {
    const Use use(*this);
    const auto* current = use.get();
    return current != nullptr ? current->projection.revision : 0;
}

bool RigEngine::monitorsInput() noexcept {
    const Use use(*this);
    const auto* current = use.get();
    return current != nullptr && !current->faded.load(std::memory_order_acquire);
}

std::uint32_t RigEngine::inputChannel() noexcept {
    const Use use(*this);
    const auto* current = use.get();
    return current != nullptr ? current->projection.audioInputChannel : 0;
}

void RigEngine::mix(const float* const* inputChannels, const int inputChannelCount,
                    float* const* outputChannels, const int outputChannelCount,
                    const int numSamples, const double sampleRate) noexcept {
    const Use use(*this);
    auto* current = use.get();
    if (current == nullptr || current->faded.load(std::memory_order_acquire) ||
        sampleRate != current->sampleRate || numSamples <= 0 || numSamples > current->blockSize)
        return;
    const auto channel = current->projection.audioInputChannel;
    const float* source =
        inputChannels != nullptr && channel < static_cast<std::uint32_t>(inputChannelCount)
            ? inputChannels[channel]
            : nullptr;
    auto& render = current->renderBuffer;
    const float* chainInput[1] = {source};
    current->chain->process(chainInput, source != nullptr ? 1 : 0, render.getArrayOfWritePointers(),
                            2, numSamples);

    const auto gain =
        juce::Decibels::decibelsToGain(static_cast<float>(current->projection.gainDb), -100.0f);
    const auto stopping = current->stopping.load(std::memory_order_acquire);
    for (int outputChannel = 0; outputChannel < outputChannelCount; ++outputChannel) {
        auto* output = outputChannels[outputChannel];
        if (output == nullptr) continue;
        const auto* rendered = render.getReadPointer(juce::jmin(outputChannel, 1));
        if (stopping) {
            for (int sample = 0; sample < numSamples; ++sample) {
                const auto fade = 1.0f - static_cast<float>(sample + 1) / numSamples;
                output[sample] += rendered[sample] * gain * fade;
            }
        } else {
            for (int sample = 0; sample < numSamples; ++sample)
                output[sample] += rendered[sample] * gain;
        }
    }
    if (stopping) current->faded.store(true, std::memory_order_release);
}

}  // namespace riffra

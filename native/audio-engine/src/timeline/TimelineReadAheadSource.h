#pragma once

#include <JuceHeader.h>

#include <algorithm>
#include <atomic>
#include <memory>
#include <utility>

namespace riffra {

class TimelineReadAheadSource final : public juce::PositionableAudioSource,
                                      private juce::TimeSliceClient {
public:
    TimelineReadAheadSource(std::unique_ptr<juce::AudioFormatReader> input,
                            juce::TimeSliceThread& worker, int capacity)
        : reader(std::move(input)), thread(worker), buffer(2, capacity), staging(2, capacity) {}

    ~TimelineReadAheadSource() override { thread.removeTimeSliceClient(this); }

    void prepareToPlay(int, double) override {
        thread.addTimeSliceClient(this);
        thread.moveToFrontOfQueue(this);
    }
    void releaseResources() override { thread.removeTimeSliceClient(this); }
    void setNextReadPosition(juce::int64 position) override {
        nextPosition.store(position, std::memory_order_release);
        thread.moveToFrontOfQueue(this);
    }
    juce::int64 getNextReadPosition() const override {
        return nextPosition.load(std::memory_order_acquire);
    }
    juce::int64 getTotalLength() const override { return reader->lengthInSamples; }
    bool isLooping() const override { return false; }
    bool failed() const noexcept { return readFailed.load(std::memory_order_acquire); }

    bool ready(int samples) noexcept {
        const juce::SpinLock::ScopedLockType guard(bufferLock);
        const auto position = getNextReadPosition();
        return position >= bufferStart && position + samples <= bufferEnd;
    }

    void getNextAudioBlock(const juce::AudioSourceChannelInfo& info) override {
        info.clearActiveBufferRegion();
        const auto position = getNextReadPosition();
        {
            const juce::SpinLock::ScopedTryLockType guard(bufferLock);
            if (guard.isLocked()) {
                if (position >= bufferStart && position + info.numSamples <= bufferEnd) {
                    const auto offset = static_cast<int>(position - bufferStart);
                    for (int channel = 0; channel < std::min(2, info.buffer->getNumChannels());
                         ++channel)
                        info.buffer->copyFrom(channel, info.startSample, buffer, channel, offset,
                                              info.numSamples);
                }
            }
        }
        nextPosition.store(position + info.numSamples, std::memory_order_release);
    }

private:
    int useTimeSlice() override {
        const auto position = getNextReadPosition();
        {
            const juce::SpinLock::ScopedLockType guard(bufferLock);
            if (position >= bufferStart && position + buffer.getNumSamples() / 2 < bufferEnd)
                return 100;
        }
        staging.clear();
        const auto remaining = std::max<juce::int64>(0, reader->lengthInSamples - position);
        const auto count =
            static_cast<int>(std::min<juce::int64>(remaining, staging.getNumSamples()));
        if (count > 0 && !reader->read(&staging, 0, count, position, true, true)) {
            readFailed.store(true, std::memory_order_release);
            return 100;
        }
        {
            const juce::SpinLock::ScopedLockType guard(bufferLock);
            std::swap(buffer, staging);
            bufferStart = position;
            bufferEnd = position + buffer.getNumSamples();
        }
        return 100;
    }

    std::unique_ptr<juce::AudioFormatReader> reader;
    juce::TimeSliceThread& thread;
    juce::AudioBuffer<float> buffer, staging;
    juce::SpinLock bufferLock;
    std::atomic<juce::int64> nextPosition{0};
    std::atomic<bool> readFailed{false};
    juce::int64 bufferStart = 0, bufferEnd = 0;
};

}  // namespace riffra

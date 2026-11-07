#pragma once

#include <JuceHeader.h>

#include <algorithm>
#include <atomic>
#include <bit>
#include <cstdint>
#include <memory>
#include <utility>

namespace riffra {

class TimelineReadAheadSource final : public juce::PositionableAudioSource,
                                      private juce::TimeSliceClient {
    static_assert(std::atomic<std::uint64_t>::is_always_lock_free);

public:
    TimelineReadAheadSource(std::unique_ptr<juce::AudioFormatReader> input,
                            juce::TimeSliceThread& worker, int capacity)
        : reader(std::move(input)),
          thread(worker),
          capacity(((capacity + 2047) / 2048) * 2048),
          samples(std::make_unique<std::atomic<std::uint64_t>[]>(this->capacity)),
          tags(std::make_unique<std::atomic<juce::int64>[]>(this->capacity / 2048)),
          staging(2, 2048) {
        for (int index = 0; index < this->capacity / 2048; ++index) tags[index].store(-1);
    }

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
        if (position >= reader->lengthInSamples) return true;
        const auto available = std::min<juce::int64>(samples, reader->lengthInSamples - position);
        return position >= bufferStart && position + available <= bufferEnd;
    }

    void getNextAudioBlock(const juce::AudioSourceChannelInfo& info) override {
        info.clearActiveBufferRegion();
        const auto position = getNextReadPosition();
        const auto available = static_cast<int>(std::min<juce::int64>(
            info.numSamples, std::max<juce::int64>(0, reader->lengthInSamples - position)));
        for (int offset = 0; offset < available;) {
            const auto frame = position + offset;
            const auto chunk = frame / 2048;
            const auto chunkOffset = static_cast<int>(frame % 2048);
            const auto count = std::min(available - offset, 2048 - chunkOffset);
            const auto slot = static_cast<int>(chunk % (capacity / 2048));
            if (tags[slot].load(std::memory_order_acquire) == chunk) {
                for (int i = 0; i < count; ++i) {
                    const auto packed =
                        samples[slot * 2048 + chunkOffset + i].load(std::memory_order_acquire);
                    info.buffer->setSample(
                        0, info.startSample + offset + i,
                        std::bit_cast<float>(static_cast<std::uint32_t>(packed)));
                    if (info.buffer->getNumChannels() > 1)
                        info.buffer->setSample(
                            1, info.startSample + offset + i,
                            std::bit_cast<float>(static_cast<std::uint32_t>(packed >> 32)));
                }
                if (tags[slot].load(std::memory_order_acquire) != chunk)
                    info.buffer->clear(info.startSample + offset, count);
            }
            offset += count;
        }
        nextPosition.store(position + info.numSamples, std::memory_order_release);
    }

private:
    int useTimeSlice() override {
        const auto position = getNextReadPosition();
        if (position >= reader->lengthInSamples) return 100;
        juce::int64 readStart;
        int requested;
        {
            const juce::SpinLock::ScopedLockType guard(bufferLock);
            if (position < bufferStart || position >= bufferEnd) {
                bufferStart = (position / 2048) * 2048;
                bufferEnd = bufferStart;
            } else {
                bufferStart = position;
            }
            readStart = bufferEnd;
            if (readStart >= reader->lengthInSamples) return 100;
            const auto space = capacity - (bufferEnd - bufferStart);
            requested = static_cast<int>(std::min<juce::int64>(space, staging.getNumSamples()));
            if (space < staging.getNumSamples()) {
                const auto delay =
                    static_cast<int>(500.0 * (bufferEnd - position) / reader->sampleRate);
                return std::clamp(delay, 1, 100);
            }
        }
        staging.clear();
        const auto remaining = std::max<juce::int64>(0, reader->lengthInSamples - readStart);
        const auto count = static_cast<int>(std::min<juce::int64>(remaining, requested));
        if (count > 0 && !reader->read(&staging, 0, count, readStart, true, true)) {
            readFailed.store(true, std::memory_order_release);
            return 100;
        }
        {
            const juce::SpinLock::ScopedLockType guard(bufferLock);
            const auto current = getNextReadPosition();
            if (current < bufferStart || current > readStart + requested) return 1;
            const auto chunk = readStart / 2048;
            const auto slot = static_cast<int>(chunk % (capacity / 2048));
            tags[slot].store(-1, std::memory_order_release);
            for (int i = 0; i < count; ++i) {
                const auto left = std::bit_cast<std::uint32_t>(staging.getSample(0, i));
                const auto right = std::bit_cast<std::uint32_t>(staging.getSample(1, i));
                samples[slot * 2048 + i].store((static_cast<std::uint64_t>(right) << 32) | left,
                                               std::memory_order_release);
            }
            tags[slot].store(chunk, std::memory_order_release);
            bufferEnd = readStart + count;
        }
        return 1;
    }

    std::unique_ptr<juce::AudioFormatReader> reader;
    juce::TimeSliceThread& thread;
    int capacity;
    std::unique_ptr<std::atomic<std::uint64_t>[]> samples;
    std::unique_ptr<std::atomic<juce::int64>[]> tags;
    juce::AudioBuffer<float> staging;
    juce::SpinLock bufferLock;
    std::atomic<juce::int64> nextPosition{0};
    std::atomic<bool> readFailed{false};
    juce::int64 bufferStart = 0, bufferEnd = 0;
};

}  // namespace riffra

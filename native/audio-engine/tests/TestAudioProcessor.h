#pragma once

#include "PluginRack.h"

#include <JuceHeader.h>

#include <cmath>
#include <memory>
#include <vector>

namespace riffra {

struct ProcessorTrace final {
    bool prepared = false;
    bool processed = false;
    bool released = false;
};

class TestProcessor final : public juce::AudioProcessor {
public:
    explicit TestProcessor(ProcessorTrace& processorTrace)
        : AudioProcessor(BusesProperties()
                             .withInput("Input", juce::AudioChannelSet::stereo(), true)
                             .withOutput("Output", juce::AudioChannelSet::stereo(), true)),
          trace(processorTrace) {}

    void prepareToPlay(double sampleRate, int samplesPerBlock) override {
        trace.prepared = sampleRate > 0.0 && samplesPerBlock > 0;
    }
    void releaseResources() override { trace.released = true; }
    bool isBusesLayoutSupported(const BusesLayout& layout) const override {
        return layout.getMainInputChannelSet() == juce::AudioChannelSet::stereo()
            && layout.getMainOutputChannelSet() == juce::AudioChannelSet::stereo();
    }
    void processBlock(juce::AudioBuffer<float>& buffer, juce::MidiBuffer&) override {
        trace.processed = trace.prepared;
        buffer.applyGain(2.0f);
    }
    juce::AudioProcessorEditor* createEditor() override { return nullptr; }
    bool hasEditor() const override { return false; }
    const juce::String getName() const override { return "Riffra Test Processor"; }
    bool acceptsMidi() const override { return false; }
    bool producesMidi() const override { return false; }
    bool isMidiEffect() const override { return false; }
    double getTailLengthSeconds() const override { return 0.0; }
    int getNumPrograms() override { return 1; }
    int getCurrentProgram() override { return 0; }
    void setCurrentProgram(int) override {}
    const juce::String getProgramName(int) override { return {}; }
    void changeProgramName(int, const juce::String&) override {}
    void getStateInformation(juce::MemoryBlock&) override {}
    void setStateInformation(const void*, int) override {}

private:
    ProcessorTrace& trace;
};

struct InstrumentTrace final {
    bool prepared = false;
    bool processed = false;
    bool released = false;
    bool noteHeld = false;
    juce::MidiMessage lastMidiMessage;
    std::vector<juce::MidiMessage> midiMessages;
    int midiMessageCount = 0;
};

class TestInstrumentProcessor final : public juce::AudioProcessor {
public:
    explicit TestInstrumentProcessor(InstrumentTrace& processorTrace)
        : AudioProcessor(BusesProperties()
                             .withOutput("Output", juce::AudioChannelSet::stereo(), true)),
          trace(processorTrace) {}

    void prepareToPlay(double sampleRate, int samplesPerBlock) override {
        trace.prepared = sampleRate > 0.0 && samplesPerBlock > 0;
    }
    void releaseResources() override { trace.released = true; }
    bool isBusesLayoutSupported(const BusesLayout& layout) const override {
        return layout.getMainInputChannelSet() == juce::AudioChannelSet::disabled()
            && layout.getMainOutputChannelSet() == juce::AudioChannelSet::stereo();
    }
    void processBlock(juce::AudioBuffer<float>& buffer, juce::MidiBuffer& midi) override {
        trace.processed = trace.prepared;
        for (const auto metadata : midi) {
            trace.lastMidiMessage = metadata.getMessage();
            trace.midiMessages.push_back(trace.lastMidiMessage);
            ++trace.midiMessageCount;
            if (trace.lastMidiMessage.isNoteOn())
                trace.noteHeld = true;
            else if (trace.lastMidiMessage.isNoteOff())
                trace.noteHeld = false;
        }
        buffer.clear();
        if (trace.noteHeld) {
            for (int channel = 0; channel < buffer.getNumChannels(); ++channel)
                juce::FloatVectorOperations::fill(
                    buffer.getWritePointer(channel), 0.25f, buffer.getNumSamples());
        }
    }
    juce::AudioProcessorEditor* createEditor() override { return nullptr; }
    bool hasEditor() const override { return false; }
    const juce::String getName() const override { return "Riffra Test Instrument"; }
    bool acceptsMidi() const override { return true; }
    bool producesMidi() const override { return false; }
    bool isMidiEffect() const override { return false; }
    double getTailLengthSeconds() const override { return 0.0; }
    int getNumPrograms() override { return 1; }
    int getCurrentProgram() override { return 0; }
    void setCurrentProgram(int) override {}
    const juce::String getProgramName(int) override { return {}; }
    void changeProgramName(int, const juce::String&) override {}
    void getStateInformation(juce::MemoryBlock&) override {}
    void setStateInformation(const void*, int) override {}

private:
    InstrumentTrace& trace;
};

struct ChainTrace final {
    int id = 0;
    float gain = 1.0f;
    double tailSeconds = 0.0;
    std::vector<int>* order = nullptr;
};

class TestChainProcessor final : public juce::AudioProcessor {
public:
    TestChainProcessor(int processorId, float processorGain, int latency,
                       std::vector<int>& processorOrder, double processorTailSeconds = 0.0)
        : AudioProcessor(BusesProperties()
                             .withInput("Input", juce::AudioChannelSet::stereo(), true)
                             .withOutput("Output", juce::AudioChannelSet::stereo(), true)),
          trace { processorId, processorGain, processorTailSeconds, &processorOrder } {
        setLatencySamples(latency);
    }

    void prepareToPlay(double, int) override {}
    void releaseResources() override {}
    bool isBusesLayoutSupported(const BusesLayout& layout) const override {
        return layout.getMainInputChannelSet() == juce::AudioChannelSet::stereo()
            && layout.getMainOutputChannelSet() == juce::AudioChannelSet::stereo();
    }
    void processBlock(juce::AudioBuffer<float>& buffer, juce::MidiBuffer&) override {
        trace.order->push_back(trace.id);
        buffer.applyGain(trace.gain);
    }
    juce::AudioProcessorEditor* createEditor() override { return nullptr; }
    bool hasEditor() const override { return false; }
    const juce::String getName() const override { return "Riffra Chain Test Processor"; }
    bool acceptsMidi() const override { return false; }
    bool producesMidi() const override { return false; }
    bool isMidiEffect() const override { return false; }
    double getTailLengthSeconds() const override { return trace.tailSeconds; }
    int getNumPrograms() override { return 1; }
    int getCurrentProgram() override { return 0; }
    void setCurrentProgram(int) override {}
    const juce::String getProgramName(int) override { return {}; }
    void changeProgramName(int, const juce::String&) override {}
    void getStateInformation(juce::MemoryBlock&) override {}
    void setStateInformation(const void*, int) override {}

private:
    ChainTrace trace;
};

class StateTestProcessor final : public juce::AudioProcessor {
public:
    StateTestProcessor()
        : AudioProcessor(BusesProperties()
                             .withInput("Input", juce::AudioChannelSet::stereo(), true)
                             .withOutput("Output", juce::AudioChannelSet::stereo(), true)) {
        parameters.reserve(700);
        for (int index = 0; index < 700; ++index) {
            auto* parameter = new juce::AudioParameterFloat(
                "state" + juce::String(index), "State " + juce::String(index),
                0.0f, 1.0f, 0.0f);
            parameters.push_back(parameter);
            addParameter(parameter);
        }
    }

    void prepareToPlay(double, int) override {}
    void releaseResources() override {}
    bool isBusesLayoutSupported(const BusesLayout& layout) const override {
        return layout.getMainInputChannelSet() == juce::AudioChannelSet::stereo()
            && layout.getMainOutputChannelSet() == juce::AudioChannelSet::stereo();
    }
    void processBlock(juce::AudioBuffer<float>&, juce::MidiBuffer&) override {}
    juce::AudioProcessorEditor* createEditor() override { return nullptr; }
    bool hasEditor() const override { return false; }
    const juce::String getName() const override { return "State Test Processor"; }
    bool acceptsMidi() const override { return false; }
    bool producesMidi() const override { return false; }
    bool isMidiEffect() const override { return false; }
    double getTailLengthSeconds() const override { return 0.0; }
    int getNumPrograms() override { return 1; }
    int getCurrentProgram() override { return 0; }
    void setCurrentProgram(int) override {}
    const juce::String getProgramName(int) override { return {}; }
    void changeProgramName(int, const juce::String&) override {}
    void getStateInformation(juce::MemoryBlock& state) override {
        for (const auto* parameter : getParameters()) {
            const auto normalized = parameter->getValue();
            state.append(&normalized, sizeof(normalized));
        }
    }
    void setStateInformation(const void* data, int size) override {
        if (data == nullptr || size != static_cast<int>(parameters.size() * sizeof(float)))
            return;
        const auto* values = static_cast<const float*>(data);
        for (std::size_t index = 0; index < parameters.size(); ++index)
            parameters[index]->setValueNotifyingHost(values[index]);
    }

private:
    std::vector<juce::AudioParameterFloat*> parameters;
};

class PluginRackTestPeer final {
public:
    static std::unique_ptr<PluginRack> install(
        std::unique_ptr<juce::AudioProcessor> processor,
        double sampleRate,
        int blockSize,
        juce::String& error) {
        if (processor == nullptr) {
            error = "Test processor was null.";
            return {};
        }
        if (const auto configurationError =
                PluginRack::configureProcessor(*processor, sampleRate, blockSize)) {
            error = configurationError->message;
            return {};
        }

        auto rack = std::make_unique<PluginRack>();
        rack->updateParameterCache(*processor);
        if (!rack->allocateParameterQueue(
                static_cast<std::size_t>(processor->getParameters().size()), error))
            return {};
        rack->pendingMidi.reset();
        rack->preparedSampleRate.store(sampleRate, std::memory_order_release);
        rack->preparedBlockSize.store(blockSize, std::memory_order_release);
        rack->pluginInputChannels.store(
            processor->getMainBusNumInputChannels(), std::memory_order_release);
        rack->pluginOutputChannels.store(
            processor->getMainBusNumOutputChannels(), std::memory_order_release);
        rack->plugin = std::move(processor);
        rack->loaded.store(true, std::memory_order_release);
        rack->loadCount.store(1, std::memory_order_release);
        return rack;
    }
};

} // namespace riffra

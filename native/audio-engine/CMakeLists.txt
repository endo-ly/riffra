cmake_minimum_required(VERSION 3.22)

project(riffra_audio VERSION 0.1.0 LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

include(FetchContent)
FetchContent_Declare(
  JUCE
  GIT_REPOSITORY https://github.com/juce-framework/JUCE.git
  GIT_TAG 8.0.13
  GIT_SHALLOW TRUE
  GIT_PROGRESS TRUE
)
FetchContent_MakeAvailable(JUCE)

juce_add_console_app(riffra-audio
  PRODUCT_NAME "Riffra Audio Engine"
  COMPANY_NAME "Riffra"
  VERSION "${PROJECT_VERSION}"
)

juce_add_console_app(riffra-plugin-scan
  PRODUCT_NAME "Riffra Plugin Scanner"
  COMPANY_NAME "Riffra"
  VERSION "${PROJECT_VERSION}"
)

juce_generate_juce_header(riffra-audio)
juce_generate_juce_header(riffra-plugin-scan)

target_sources(riffra-audio PRIVATE
  src/Main.cpp
  src/SafetyAudioCallback.cpp
  src/SafetyAudioCallback.h
  src/RecordingSession.cpp
  src/RecordingSession.h
  src/PluginRack.cpp
  src/PluginRack.h
  src/RecordingSelfTest.cpp
  src/RecordingSelfTest.h
)

target_sources(riffra-plugin-scan PRIVATE
  src/PluginScanner.cpp
)

target_compile_definitions(riffra-audio PRIVATE
  JUCE_ASIO=1
  JUCE_PLUGINHOST_VST3=1
  JUCE_USE_CURL=0
  JUCE_WEB_BROWSER=0
)

target_compile_definitions(riffra-plugin-scan PRIVATE
  JUCE_PLUGINHOST_VST3=1
  JUCE_USE_CURL=0
  JUCE_WEB_BROWSER=0
)

target_link_libraries(riffra-audio PRIVATE
  juce::juce_audio_devices
  juce::juce_audio_formats
  juce::juce_audio_processors
  juce::juce_audio_utils
  juce::juce_core
  juce::juce_dsp
  juce::juce_events
  juce::juce_recommended_config_flags
  juce::juce_recommended_lto_flags
  juce::juce_recommended_warning_flags
)

target_link_libraries(riffra-plugin-scan PRIVATE
  juce::juce_audio_processors
  juce::juce_core
  juce::juce_events
  juce::juce_recommended_config_flags
  juce::juce_recommended_lto_flags
  juce::juce_recommended_warning_flags
)

if(MSVC)
  target_compile_options(riffra-audio PRIVATE /W4 /permissive- /EHsc /utf-8)
  target_compile_options(riffra-plugin-scan PRIVATE /W4 /permissive- /EHsc /utf-8)
endif()

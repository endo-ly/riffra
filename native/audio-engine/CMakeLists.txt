cmake_minimum_required(VERSION 3.22)

project(riffra_audio VERSION 0.1.0 LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

include(FetchContent)
include(CTest)

FetchContent_Declare(
  JUCE
  GIT_REPOSITORY https://github.com/juce-framework/JUCE.git
  GIT_TAG 8.0.14
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

add_library(riffra-audio-core STATIC)

target_sources(riffra-audio-core PRIVATE
  src/SafetyAudioCallback.cpp
  src/SafetyAudioCallback.h
  src/TimelineEngine.cpp
  src/TimelineEngine.h
  src/TimelineTimebase.h
  src/RecordingCaptureRuntime.cpp
  src/RecordingCaptureRuntime.h
  src/ArrangementGraph.cpp
  src/ArrangementGraph.h
  src/OfflineRenderer.cpp
  src/OfflineRenderer.h
  src/AudioSafetyDsp.h
  src/AudioRuntimeStatus.cpp
  src/AudioRuntimeStatus.h
  src/RecordingSession.cpp
  src/RecordingSession.h
  src/ArrangeRecordingSession.cpp
  src/ArrangeRecordingSession.h
  src/ArrangementCaptureSink.h
  src/PluginRack.cpp
  src/PluginRack.h
  src/PluginChain.cpp
  src/PluginChain.h
  src/PluginEditorHost.cpp
  src/PluginEditorHost.h
)

target_sources(riffra-audio PRIVATE
  src/Main.cpp
)

target_sources(riffra-plugin-scan PRIVATE
  src/PluginScanner.cpp
)

set(RIFFRA_AUDIO_JUCE_DEFINITIONS
  JUCE_ASIO=1
  JUCE_PLUGINHOST_VST3=1
  JUCE_USE_CURL=0
  JUCE_WEB_BROWSER=0
)

set(RIFFRA_PLUGIN_SCAN_JUCE_DEFINITIONS
  JUCE_PLUGINHOST_VST3=1
  JUCE_USE_CURL=0
  JUCE_WEB_BROWSER=0
)

target_compile_definitions(riffra-audio-core PUBLIC
  ${RIFFRA_AUDIO_JUCE_DEFINITIONS}
)

# riffra-audio only consumes the public interface of riffra-audio-core, so the
# JUCE compile definitions and libraries are already propagated.
target_compile_definitions(riffra-plugin-scan PRIVATE
  ${RIFFRA_PLUGIN_SCAN_JUCE_DEFINITIONS}
)

set(RIFFRA_AUDIO_JUCE_LIBRARIES
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

set(RIFFRA_PLUGIN_SCAN_JUCE_LIBRARIES
  juce::juce_audio_processors
  juce::juce_core
  juce::juce_events
  juce::juce_recommended_config_flags
  juce::juce_recommended_lto_flags
  juce::juce_recommended_warning_flags
)

target_link_libraries(riffra-audio-core PUBLIC
  ${RIFFRA_AUDIO_JUCE_LIBRARIES}
)

target_link_libraries(riffra-audio PRIVATE
  riffra-audio-core
)

target_link_libraries(riffra-plugin-scan PRIVATE
  ${RIFFRA_PLUGIN_SCAN_JUCE_LIBRARIES}
)

juce_generate_juce_header(riffra-audio)
juce_generate_juce_header(riffra-plugin-scan)

target_include_directories(riffra-audio-core PUBLIC
  ${CMAKE_CURRENT_SOURCE_DIR}/src
)

if(MSVC)
  set(RIFFRA_MSVC_WARNINGS /W4 /permissive- /EHsc /utf-8)
  foreach(target IN ITEMS riffra-audio-core riffra-audio riffra-plugin-scan)
    target_compile_options(${target} PRIVATE ${RIFFRA_MSVC_WARNINGS})
  endforeach()
endif()

# The existing production headers use JUCE's generated JuceHeader.h. JUCE only
# generates that header for its app targets, so generate a matching shared
# header for the reusable core library from the audio target's module list.
get_target_property(RIFFRA_AUDIO_JUCE_DEFS_FILE riffra-audio JUCE_DEFS_FILE)
set(RIFFRA_AUDIO_CORE_JUCE_DIRECTORY
  ${CMAKE_CURRENT_BINARY_DIR}/riffra-audio-core_artefacts/JuceLibraryCode)
set(RIFFRA_AUDIO_CORE_JUCE_HEADER
  ${RIFFRA_AUDIO_CORE_JUCE_DIRECTORY}/JuceHeader.h)

add_custom_command(
  OUTPUT ${RIFFRA_AUDIO_CORE_JUCE_HEADER}
  COMMAND ${CMAKE_COMMAND} -E make_directory ${RIFFRA_AUDIO_CORE_JUCE_DIRECTORY}
  COMMAND juce::juceaide header
    ${RIFFRA_AUDIO_JUCE_DEFS_FILE}
    ${RIFFRA_AUDIO_CORE_JUCE_HEADER}
  DEPENDS ${RIFFRA_AUDIO_JUCE_DEFS_FILE}
  VERBATIM
)

add_custom_target(riffra-audio-core-juce-header
  DEPENDS ${RIFFRA_AUDIO_CORE_JUCE_HEADER})
add_dependencies(riffra-audio-core riffra-audio-core-juce-header)
target_sources(riffra-audio-core PRIVATE ${RIFFRA_AUDIO_CORE_JUCE_HEADER})
target_include_directories(riffra-audio-core PUBLIC
  ${RIFFRA_AUDIO_CORE_JUCE_DIRECTORY}
)

if(BUILD_TESTING)
  FetchContent_Declare(
    googletest
    GIT_REPOSITORY https://github.com/google/googletest.git
    GIT_TAG v1.17.0
    GIT_SHALLOW TRUE
  )
  FetchContent_MakeAvailable(googletest)

  add_executable(riffra-audio-tests)
  target_sources(riffra-audio-tests PRIVATE
    tests/ArrangementGraphTest.cpp
    tests/AudioSafetyDspTest.cpp
    tests/SafetyAudioCallbackTest.cpp
    tests/PluginRackTest.cpp
    tests/PluginChainTest.cpp
    tests/RecordingSessionTest.cpp
    tests/ArrangeRecordingSessionTest.cpp
    tests/TimelineEngineTest.cpp
  )
  target_link_libraries(riffra-audio-tests PRIVATE
    riffra-audio-core
    GTest::gtest_main
  )
  # JUCE definitions come through riffra-audio-core's PUBLIC interface.
  if(MSVC)
    target_compile_options(riffra-audio-tests PRIVATE ${RIFFRA_MSVC_WARNINGS})
  endif()

include(GoogleTest)
gtest_discover_tests(riffra-audio-tests)
endif()

# Sidecar names must match Tauri's externalBin resolution:
# binaries/<name>-<target-triple>(.exe). Default to Windows MSVC target;
# override with -DRIFFRA_TARGET_TRIPLE=<triple> when cross-compiling.
if(NOT DEFINED RIFFRA_TARGET_TRIPLE)
    set(RIFFRA_TARGET_TRIPLE "x86_64-pc-windows-msvc")
endif()
if(CMAKE_SYSTEM_NAME STREQUAL "Windows")
    set(RIFFRA_BINARY_SUFFIX ".exe")
else()
    set(RIFFRA_BINARY_SUFFIX "")
endif()

foreach(sidecar IN ITEMS riffra-audio riffra-plugin-scan)
  install(FILES $<TARGET_FILE:${sidecar}>
      DESTINATION "src-tauri/binaries"
      RENAME "${sidecar}-${RIFFRA_TARGET_TRIPLE}${RIFFRA_BINARY_SUFFIX}"
      COMPONENT riffra-sidecars
  )
endforeach()

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

juce_add_console_app(riffra-render
  PRODUCT_NAME "Riffra Offline Renderer"
  COMPANY_NAME "Riffra"
  VERSION "${PROJECT_VERSION}"
)

add_library(riffra-render-core STATIC)
add_library(riffra-audio-core STATIC)

target_sources(riffra-render-core PRIVATE
  src/processing/TimelineEngine.cpp
  src/processing/TimelineEngine.h
  src/processing/TimelineTimebase.h
  src/processing/RecordingCaptureRuntime.cpp
  src/processing/RecordingCaptureRuntime.h
  src/processing/ArrangementGraph.cpp
  src/processing/ArrangementGraph.h
  src/processing/OfflineRenderer.cpp
  src/processing/OfflineRenderer.h
  src/processing/ArrangementCaptureSink.h
  src/processing/PluginRack.cpp
  src/processing/PluginRack.h
  src/processing/PluginChain.cpp
  src/processing/PluginChain.h
  src/realtime/FaultInjection.cpp
  src/realtime/FaultInjection.h
)

target_sources(riffra-audio-core PRIVATE
  src/realtime/SafetyAudioCallback.cpp
  src/realtime/SafetyAudioCallback.h
  src/realtime/AudioSafetyDsp.h
  src/realtime/AudioRuntimeStatus.cpp
  src/realtime/AudioRuntimeStatus.h
  src/realtime/RecordingSession.cpp
  src/realtime/RecordingSession.h
  src/realtime/ArrangeRecordingSession.cpp
  src/realtime/ArrangeRecordingSession.h
  src/realtime/PluginEditorHost.cpp
  src/realtime/PluginEditorHost.h
  src/realtime/RuntimeLifecycleExecutor.cpp
  src/realtime/RuntimeLifecycleExecutor.h
)

target_sources(riffra-audio PRIVATE
  src/AudioMain.cpp
)

target_sources(riffra-plugin-scan PRIVATE
  src/PluginScannerMain.cpp
)

target_sources(riffra-render PRIVATE
  src/RenderMain.cpp
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

set(RIFFRA_RENDER_JUCE_DEFINITIONS
  JUCE_PLUGINHOST_VST3=1
  JUCE_USE_CURL=0
  JUCE_WEB_BROWSER=0
)

target_compile_definitions(riffra-render-core PUBLIC
  ${RIFFRA_RENDER_JUCE_DEFINITIONS}
)

target_compile_definitions(riffra-audio-core PRIVATE
  JUCE_ASIO=1
)

target_compile_definitions(riffra-audio PRIVATE
  ${RIFFRA_AUDIO_JUCE_DEFINITIONS}
)

target_compile_definitions(riffra-plugin-scan PRIVATE
  ${RIFFRA_PLUGIN_SCAN_JUCE_DEFINITIONS}
)

set(RIFFRA_RENDER_JUCE_LIBRARIES
  juce::juce_audio_formats
  juce::juce_audio_processors
  juce::juce_core
  juce::juce_dsp
  juce::juce_events
  juce::juce_recommended_config_flags
  juce::juce_recommended_lto_flags
  juce::juce_recommended_warning_flags
)

set(RIFFRA_AUDIO_JUCE_LIBRARIES
  juce::juce_audio_devices
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

target_link_libraries(riffra-render-core PUBLIC
  ${RIFFRA_RENDER_JUCE_LIBRARIES}
)

target_link_libraries(riffra-audio-core PUBLIC
  riffra-render-core
  ${RIFFRA_AUDIO_JUCE_LIBRARIES}
)

target_link_libraries(riffra-audio PRIVATE
  riffra-audio-core
)

target_link_libraries(riffra-plugin-scan PRIVATE
  ${RIFFRA_PLUGIN_SCAN_JUCE_LIBRARIES}
)

target_link_libraries(riffra-render PRIVATE
  riffra-render-core
)

juce_generate_juce_header(riffra-audio)
juce_generate_juce_header(riffra-plugin-scan)
juce_generate_juce_header(riffra-render)

target_include_directories(riffra-render-core PUBLIC
  ${CMAKE_CURRENT_SOURCE_DIR}/src/processing
  ${CMAKE_CURRENT_SOURCE_DIR}/src/realtime
)

target_include_directories(riffra-audio-core PUBLIC
  ${CMAKE_CURRENT_SOURCE_DIR}/src/realtime
)

if(MSVC)
  set(RIFFRA_MSVC_WARNINGS /W4 /permissive- /EHsc /utf-8)
  foreach(target IN ITEMS
      riffra-render-core
      riffra-audio-core
      riffra-audio
      riffra-plugin-scan
      riffra-render)
    target_compile_options(${target} PRIVATE ${RIFFRA_MSVC_WARNINGS})
  endforeach()
endif()

function(riffra_generate_core_juce_header core_target app_target)
  get_target_property(juce_defs_file ${app_target} JUCE_DEFS_FILE)
  set(juce_directory
    ${CMAKE_CURRENT_BINARY_DIR}/${core_target}_artefacts/JuceLibraryCode)
  set(juce_header ${juce_directory}/JuceHeader.h)
  set(header_target ${core_target}-juce-header)

  add_custom_command(
    OUTPUT ${juce_header}
    COMMAND ${CMAKE_COMMAND} -E make_directory ${juce_directory}
    COMMAND juce::juceaide header ${juce_defs_file} ${juce_header}
    DEPENDS ${juce_defs_file}
    VERBATIM
  )
  add_custom_target(${header_target} DEPENDS ${juce_header})
  add_dependencies(${core_target} ${header_target})
  target_sources(${core_target} PRIVATE ${juce_header})
  target_include_directories(${core_target} PUBLIC ${juce_directory})
endfunction()

riffra_generate_core_juce_header(riffra-render-core riffra-render)
riffra_generate_core_juce_header(riffra-audio-core riffra-audio)

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
    tests/RuntimeLifecycleExecutorTest.cpp
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

foreach(sidecar IN ITEMS riffra-audio riffra-plugin-scan riffra-render)
  install(FILES $<TARGET_FILE:${sidecar}>
      DESTINATION "apps/desktop/src-tauri/binaries"
      RENAME "${sidecar}-${RIFFRA_TARGET_TRIPLE}${RIFFRA_BINARY_SUFFIX}"
      COMPONENT riffra-sidecars
  )
endforeach()

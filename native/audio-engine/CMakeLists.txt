cmake_minimum_required(VERSION 3.22)

project(riffra_audio VERSION 0.1.0 LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

include(FetchContent)
include(CTest)

set(RIFFRA_SONALLOY_VERSION "2026.9.5" CACHE STRING
  "Pinned Sonalloy release version used for the built-in instrument engine and resources")
set(RIFFRA_SONALLOY_RELEASE_TAG "v${RIFFRA_SONALLOY_VERSION}")

FetchContent_Declare(
  JUCE
  GIT_REPOSITORY https://github.com/juce-framework/JUCE.git
  GIT_TAG 9.0.0
  GIT_SHALLOW TRUE
  GIT_PROGRESS TRUE
)
FetchContent_MakeAvailable(JUCE)

FetchContent_Declare(
  SONALLOY
  GIT_REPOSITORY https://github.com/endo-ly/sonalloy.git
  GIT_TAG "${RIFFRA_SONALLOY_RELEASE_TAG}"
  GIT_SHALLOW TRUE
  GIT_PROGRESS TRUE
)
FetchContent_MakeAvailable(SONALLOY)

set(RIFFRA_SONALLOY_CARGO_TARGET_DIR
  "${CMAKE_CURRENT_BINARY_DIR}/sonalloy-cargo-target")
set(RIFFRA_SONALLOY_CARGO_PROFILE "" CACHE STRING
  "Cargo profile used to build the Sonalloy C API (dev or release)")
if(NOT RIFFRA_SONALLOY_CARGO_PROFILE)
  if(CMAKE_BUILD_TYPE STREQUAL "Debug")
    set(RIFFRA_SONALLOY_CARGO_PROFILE "dev")
  else()
    set(RIFFRA_SONALLOY_CARGO_PROFILE "release")
  endif()
endif()
if(NOT RIFFRA_SONALLOY_CARGO_PROFILE STREQUAL "dev" AND
   NOT RIFFRA_SONALLOY_CARGO_PROFILE STREQUAL "release")
  message(FATAL_ERROR
    "RIFFRA_SONALLOY_CARGO_PROFILE must be either dev or release")
endif()
if(RIFFRA_SONALLOY_CARGO_PROFILE STREQUAL "dev")
  set(RIFFRA_SONALLOY_CARGO_PROFILE_DIRECTORY "debug")
  set(RIFFRA_SONALLOY_CARGO_BUILD_PROFILE_FLAG)
  set(RIFFRA_SONALLOY_MSVC_RUNTIME "debug")
else()
  set(RIFFRA_SONALLOY_CARGO_PROFILE_DIRECTORY "release")
  set(RIFFRA_SONALLOY_CARGO_BUILD_PROFILE_FLAG --release)
  set(RIFFRA_SONALLOY_MSVC_RUNTIME "release")
endif()
if(MSVC)
  set(RIFFRA_SONALLOY_STATIC_LIBRARY
    "${CMAKE_CURRENT_BINARY_DIR}/sonalloy-capi/${RIFFRA_SONALLOY_CARGO_PROFILE_DIRECTORY}/sonalloy_capi.lib")
  set(RIFFRA_SONALLOY_CARGO_OUTPUT
    "${RIFFRA_SONALLOY_CARGO_TARGET_DIR}/${RIFFRA_SONALLOY_CARGO_PROFILE_DIRECTORY}/sonalloy_capi.lib")
else()
  set(RIFFRA_SONALLOY_STATIC_LIBRARY
    "${CMAKE_CURRENT_BINARY_DIR}/sonalloy-capi/${RIFFRA_SONALLOY_CARGO_PROFILE_DIRECTORY}/libsonalloy_capi.a")
  set(RIFFRA_SONALLOY_CARGO_OUTPUT
    "${RIFFRA_SONALLOY_CARGO_TARGET_DIR}/${RIFFRA_SONALLOY_CARGO_PROFILE_DIRECTORY}/libsonalloy_capi.a")
endif()
add_custom_command(
  OUTPUT "${RIFFRA_SONALLOY_STATIC_LIBRARY}"
  COMMAND ${CMAKE_COMMAND} -E make_directory
    "${CMAKE_CURRENT_BINARY_DIR}/sonalloy-capi/${RIFFRA_SONALLOY_CARGO_PROFILE_DIRECTORY}"
  COMMAND ${CMAKE_COMMAND} -E env
    "SONALLOY_DSP_MSVC_RUNTIME=${RIFFRA_SONALLOY_MSVC_RUNTIME}"
    cargo rustc
    --manifest-path "${sonalloy_SOURCE_DIR}/crates/sonalloy-capi/Cargo.toml"
    --package sonalloy-capi
    ${RIFFRA_SONALLOY_CARGO_BUILD_PROFILE_FLAG}
    --lib
    --crate-type staticlib
    --target-dir "${RIFFRA_SONALLOY_CARGO_TARGET_DIR}"
  COMMAND ${CMAKE_COMMAND} -E copy_if_different
    "${RIFFRA_SONALLOY_CARGO_OUTPUT}"
    "${RIFFRA_SONALLOY_STATIC_LIBRARY}"
  DEPENDS
    "${sonalloy_SOURCE_DIR}/Cargo.lock"
    "${sonalloy_SOURCE_DIR}/crates/sonalloy-capi/Cargo.toml"
  VERBATIM
)
add_custom_target(riffra-sonalloy-capi-build
  DEPENDS "${RIFFRA_SONALLOY_STATIC_LIBRARY}")
add_library(sonalloy-capi STATIC IMPORTED GLOBAL)
set_target_properties(sonalloy-capi PROPERTIES
  IMPORTED_LOCATION "${RIFFRA_SONALLOY_STATIC_LIBRARY}"
  INTERFACE_INCLUDE_DIRECTORIES
    "${sonalloy_SOURCE_DIR}/crates/sonalloy-capi/include")
add_dependencies(sonalloy-capi riffra-sonalloy-capi-build)
if(MSVC)
  target_link_libraries(sonalloy-capi INTERFACE bcrypt advapi32 ntdll userenv ws2_32)
elseif(APPLE)
  target_link_libraries(sonalloy-capi INTERFACE c++)
elseif(UNIX)
  target_link_libraries(sonalloy-capi INTERFACE stdc++ m dl pthread)
endif()

set(RIFFRA_BUILTIN_RESOURCE_ROOT
  "${CMAKE_CURRENT_BINARY_DIR}/riffra-resources/instruments/builtin")
add_custom_target(riffra-builtin-resource-stage ALL
  COMMAND ${CMAKE_COMMAND}
    -DSOURCE_PRESETS=${sonalloy_SOURCE_DIR}/presets
    -DDESTINATION=${RIFFRA_BUILTIN_RESOURCE_ROOT}
    -DSOURCE_RELEASE=${RIFFRA_SONALLOY_RELEASE_TAG}
    -P "${CMAKE_CURRENT_SOURCE_DIR}/cmake/stage_builtin_resources.cmake"
  VERBATIM
)

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
  src/processing/TimelineSnapshotBuilder.cpp
  src/processing/TimelineSnapshotBuilder.h
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
  src/processing/instrument/InstrumentRuntime.h
  src/processing/instrument/Vst3InstrumentRuntime.cpp
  src/processing/instrument/Vst3InstrumentRuntime.h
  src/processing/instrument/SonalloyInstrumentRuntime.cpp
  src/processing/instrument/SonalloyInstrumentRuntime.h
  src/processing/BoundedMpmcQueue.h
  src/processing/PluginChain.cpp
  src/processing/PluginChain.h
  src/realtime/FaultInjection.cpp
  src/realtime/FaultInjection.h
)

target_sources(riffra-audio-core PRIVATE
  src/AudioProtocol.cpp
  src/AudioProtocol.h
  src/AudioDeviceService.cpp
  src/AudioDeviceService.h
  src/MidiInputService.cpp
  src/MidiInputService.h
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
  src/processing/PluginRack.cpp
  src/processing/PluginRack.h
  src/realtime/FaultInjection.cpp
  src/realtime/FaultInjection.h
)

target_sources(riffra-render PRIVATE
  src/RenderMain.cpp
)

set(RIFFRA_AUDIO_PLATFORM_DEFINITIONS)
if(CMAKE_SYSTEM_NAME STREQUAL "Windows")
  list(APPEND RIFFRA_AUDIO_PLATFORM_DEFINITIONS JUCE_ASIO=1)
elseif(CMAKE_SYSTEM_NAME STREQUAL "Linux")
  list(APPEND RIFFRA_AUDIO_PLATFORM_DEFINITIONS
    JUCE_ALSA=1
    JUCE_JACK=0
  )
endif()

set(RIFFRA_COMMON_JUCE_DEFINITIONS
  JUCE_PLUGINHOST_VST3=1
  JUCE_PLUGINHOST_LADSPA=0
  JUCE_USE_CURL=0
  JUCE_WEB_BROWSER=0
)

set(RIFFRA_AUDIO_JUCE_DEFINITIONS
  ${RIFFRA_AUDIO_PLATFORM_DEFINITIONS}
  ${RIFFRA_COMMON_JUCE_DEFINITIONS}
)

set(RIFFRA_PLUGIN_SCAN_JUCE_DEFINITIONS
  ${RIFFRA_COMMON_JUCE_DEFINITIONS}
)

set(RIFFRA_RENDER_JUCE_DEFINITIONS
  ${RIFFRA_COMMON_JUCE_DEFINITIONS}
)

target_compile_definitions(riffra-render-core PUBLIC
  ${RIFFRA_RENDER_JUCE_DEFINITIONS}
)

target_compile_definitions(riffra-audio-core PRIVATE
  ${RIFFRA_AUDIO_PLATFORM_DEFINITIONS}
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
  sonalloy-capi
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
  ${sonalloy_SOURCE_DIR}/crates/sonalloy-capi/include
)

add_dependencies(riffra-render-core riffra-builtin-resource-stage)

target_include_directories(riffra-plugin-scan PRIVATE
  ${CMAKE_CURRENT_SOURCE_DIR}/src/processing
  ${CMAKE_CURRENT_SOURCE_DIR}/src/realtime
)

target_include_directories(riffra-audio-core PUBLIC
  ${CMAKE_CURRENT_SOURCE_DIR}/src
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
  juce_add_plugin(riffra-test-effect
    COMPANY_NAME Riffra
    IS_SYNTH FALSE
    NEEDS_MIDI_INPUT FALSE
    NEEDS_MIDI_OUTPUT FALSE
    FORMATS VST3
    PLUGIN_MANUFACTURER_CODE Rifr
    PLUGIN_CODE Tste
    PRODUCT_NAME "Riffra Test Effect"
    NEEDS_WEB_BROWSER FALSE
    NEEDS_CURL FALSE)
  target_sources(riffra-test-effect PRIVATE
    tests/TestVstPlugin.cpp)
  target_compile_definitions(riffra-test-effect PRIVATE
    JUCE_VST3_CAN_REPLACE_VST2=0
    JUCE_USE_CURL=0
    JUCE_WEB_BROWSER=0)
  target_link_libraries(riffra-test-effect PRIVATE
    juce::juce_audio_utils
    juce::juce_recommended_config_flags
    juce::juce_recommended_lto_flags
    juce::juce_recommended_warning_flags)

  juce_add_plugin(riffra-test-instrument
    COMPANY_NAME Riffra
    IS_SYNTH TRUE
    NEEDS_MIDI_INPUT TRUE
    NEEDS_MIDI_OUTPUT FALSE
    FORMATS VST3
    PLUGIN_MANUFACTURER_CODE Rifr
    PLUGIN_CODE Tsti
    PRODUCT_NAME "Riffra Test Instrument"
    NEEDS_WEB_BROWSER FALSE
    NEEDS_CURL FALSE)
  target_sources(riffra-test-instrument PRIVATE
    tests/TestVstPlugin.cpp)
  target_compile_definitions(riffra-test-instrument PRIVATE
    JUCE_VST3_CAN_REPLACE_VST2=0
    JUCE_USE_CURL=0
    JUCE_WEB_BROWSER=0)
  target_link_libraries(riffra-test-instrument PRIVATE
    juce::juce_audio_utils
    juce::juce_recommended_config_flags
    juce::juce_recommended_lto_flags
    juce::juce_recommended_warning_flags)

  add_executable(riffra-real-vst-runtime-test
    tests/RealVstRuntimeTest.cpp)
  target_link_libraries(riffra-real-vst-runtime-test PRIVATE
    riffra-render-core)
  add_dependencies(riffra-real-vst-runtime-test
    riffra-test-effect_VST3
    riffra-test-instrument_VST3)
  if(MSVC)
    target_compile_options(riffra-real-vst-runtime-test PRIVATE ${RIFFRA_MSVC_WARNINGS})
  endif()

  get_target_property(RIFFRA_TEST_EFFECT_BUNDLE
    riffra-test-effect_VST3 JUCE_PLUGIN_ARTEFACT_FILE)
  get_target_property(RIFFRA_TEST_INSTRUMENT_BUNDLE
    riffra-test-instrument_VST3 JUCE_PLUGIN_ARTEFACT_FILE)

  add_test(
    NAME PluginScannerTest.RiffraTestEffect
    COMMAND riffra-plugin-scan --validate-load "${RIFFRA_TEST_EFFECT_BUNDLE}")
  add_test(
    NAME PluginScannerTest.RiffraTestInstrument
    COMMAND riffra-plugin-scan --validate-load "${RIFFRA_TEST_INSTRUMENT_BUNDLE}")

  add_test(
    NAME RealVstRuntimeTest.LoadsMultipleRuntimeRoles
    COMMAND riffra-real-vst-runtime-test
      "${RIFFRA_TEST_EFFECT_BUNDLE}"
      "${RIFFRA_TEST_INSTRUMENT_BUNDLE}")

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
    tests/BoundedMpmcQueueTest.cpp
    tests/RuntimeLifecycleExecutorTest.cpp
    tests/TimelineEngineTest.cpp
    tests/SonalloyInstrumentRuntimeTest.cpp
    tests/AudioProtocolTest.cpp
  )
  target_link_libraries(riffra-audio-tests PRIVATE
    riffra-audio-core
    GTest::gtest_main
  )
  target_compile_definitions(riffra-audio-tests PRIVATE
    RIFFRA_SONALLOY_TEST_PRESET_ROOT="${RIFFRA_BUILTIN_RESOURCE_ROOT}"
    RIFFRA_SONALLOY_TEST_SOURCE_RELEASE="${RIFFRA_SONALLOY_RELEASE_TAG}"
    RIFFRA_SONALLOY_TEST_SOURCE_ROOT="${sonalloy_SOURCE_DIR}")
  add_dependencies(riffra-audio-tests riffra-builtin-resource-stage)
  if(UNIX AND NOT APPLE)
    target_compile_definitions(riffra-audio-tests PRIVATE RIFFRA_TRACK_MALLOC=1)
    target_link_options(riffra-audio-tests PRIVATE "-Wl,--wrap=malloc")
  endif()
  # JUCE definitions come through riffra-audio-core's PUBLIC interface.
  if(MSVC)
    target_compile_options(riffra-audio-tests PRIVATE ${RIFFRA_MSVC_WARNINGS})
  endif()

include(GoogleTest)
gtest_discover_tests(riffra-audio-tests)
endif()

# Sidecar names must match Tauri's externalBin resolution:
# binaries/<name>-<target-triple>(.exe). The default follows the host platform;
# override with -DRIFFRA_TARGET_TRIPLE=<triple> when cross-compiling.
if(NOT DEFINED RIFFRA_TARGET_TRIPLE)
    set(RIFFRA_TARGET_PROCESSOR "${CMAKE_SYSTEM_PROCESSOR}")
    if(NOT RIFFRA_TARGET_PROCESSOR)
        set(RIFFRA_TARGET_PROCESSOR "${CMAKE_HOST_SYSTEM_PROCESSOR}")
    endif()

    if(CMAKE_SYSTEM_NAME STREQUAL "Windows")
        set(RIFFRA_TARGET_TRIPLE "x86_64-pc-windows-msvc")
    elseif(CMAKE_SYSTEM_NAME STREQUAL "Linux")
        if(RIFFRA_TARGET_PROCESSOR MATCHES "^(x86_64|AMD64|amd64)$")
            set(RIFFRA_TARGET_TRIPLE "x86_64-unknown-linux-gnu")
        elseif(RIFFRA_TARGET_PROCESSOR MATCHES "^(aarch64|ARM64|arm64)$")
            set(RIFFRA_TARGET_TRIPLE "aarch64-unknown-linux-gnu")
        else()
            message(FATAL_ERROR
                "Set RIFFRA_TARGET_TRIPLE for unsupported Linux processor: ${RIFFRA_TARGET_PROCESSOR}")
        endif()
    elseif(CMAKE_SYSTEM_NAME STREQUAL "Darwin")
        if(RIFFRA_TARGET_PROCESSOR MATCHES "^(aarch64|ARM64|arm64)$")
            set(RIFFRA_TARGET_TRIPLE "aarch64-apple-darwin")
        else()
            set(RIFFRA_TARGET_TRIPLE "x86_64-apple-darwin")
        endif()
    else()
        message(FATAL_ERROR
            "Set RIFFRA_TARGET_TRIPLE for unsupported platform: ${CMAKE_SYSTEM_NAME}")
    endif()
endif()
if(CMAKE_SYSTEM_NAME STREQUAL "Windows")
    set(RIFFRA_BINARY_SUFFIX ".exe")
else()
    set(RIFFRA_BINARY_SUFFIX "")
endif()

# The repository build scripts also install unsuffixed copies beside the
# headless CLI artifact. A distribution can disable this by leaving the
# destination empty and still install the Tauri bundle copies below.
set(RIFFRA_HEADLESS_BINARIES_DESTINATION "" CACHE STRING
  "Relative install destination for unsuffixed headless sidecars")

set(RIFFRA_HEADLESS_RESOURCES_DESTINATION "" CACHE STRING
  "Relative install destination for headless built-in instrument resources")

foreach(sidecar IN ITEMS riffra-audio riffra-plugin-scan riffra-render)
  install(FILES $<TARGET_FILE:${sidecar}>
      DESTINATION "apps/desktop/src-tauri/binaries"
      RENAME "${sidecar}-${RIFFRA_TARGET_TRIPLE}${RIFFRA_BINARY_SUFFIX}"
      COMPONENT riffra-sidecars
  )
  if(RIFFRA_HEADLESS_BINARIES_DESTINATION)
    install(FILES $<TARGET_FILE:${sidecar}>
        DESTINATION "${RIFFRA_HEADLESS_BINARIES_DESTINATION}"
        RENAME "${sidecar}${RIFFRA_BINARY_SUFFIX}"
        COMPONENT riffra-sidecars
    )
  endif()
endforeach()

install(DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/riffra-resources/instruments/builtin/"
    DESTINATION "apps/desktop/src-tauri/resources/instruments/builtin"
    COMPONENT riffra-sidecars
)
if(RIFFRA_HEADLESS_RESOURCES_DESTINATION)
  install(DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/riffra-resources/instruments/builtin/"
      DESTINATION "${RIFFRA_HEADLESS_RESOURCES_DESTINATION}/riffra-resources/instruments/builtin"
      COMPONENT riffra-sidecars
  )
endif()
install(FILES
    "${sonalloy_SOURCE_DIR}/THIRD_PARTY_NOTICES.md"
    "${sonalloy_SOURCE_DIR}/LICENSE-MIT"
    "${sonalloy_SOURCE_DIR}/LICENSE-APACHE"
    DESTINATION "apps/desktop/src-tauri/resources"
    COMPONENT riffra-sidecars
)
if(RIFFRA_HEADLESS_RESOURCES_DESTINATION)
  install(FILES
      "${sonalloy_SOURCE_DIR}/THIRD_PARTY_NOTICES.md"
      "${sonalloy_SOURCE_DIR}/LICENSE-MIT"
      "${sonalloy_SOURCE_DIR}/LICENSE-APACHE"
      DESTINATION "${RIFFRA_HEADLESS_RESOURCES_DESTINATION}/riffra-resources"
      COMPONENT riffra-sidecars
  )
endif()

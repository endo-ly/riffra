# Renders a timeline whose graph hosts a VST3 that writes to the process
# standard output, and requires the worker protocol to stay on standard output
# alone while the plugin output appears on standard error.
#
# Usage:
#   cmake -DRENDER=<riffra-render> -DBUNDLE=<test vst3>
#         -DDESTINATION=<wave path> -P RenderProtocolIsolationTest.cmake

foreach(required RENDER BUNDLE DESTINATION)
  if(NOT DEFINED ${required})
    message(FATAL_ERROR "${required} is required")
  endif()
endforeach()

file(TO_CMAKE_PATH "${RENDER}" RENDER)
file(TO_CMAKE_PATH "${BUNDLE}" BUNDLE)
file(TO_CMAKE_PATH "${DESTINATION}" DESTINATION)

set(request_template [=[
{
  "type": "renderTimelineOffline",
  "protocolVersion": 3,
  "request": {
    "graph": {
      "timebase": {"ppq": 960, "bpm": 120.0, "timeSignatureNumerator": 4, "timeSignatureDenominator": 4},
      "loopRange": {"enabled": false, "startTick": 0, "endTick": 0},
      "punchRange": null,
      "metronomeEnabled": false,
      "masterGainDb": 0.0,
      "tracks": [
        {
          "id": "track-isolation",
          "kind": "audio",
          "gainDb": 0.0,
          "pan": 0.0,
          "muted": false,
          "solo": false,
          "armed": false,
          "monitorInput": false,
          "audioInput": null,
          "midiInput": {"deviceId": null, "channel": null},
          "volumeAutomation": [],
          "panAutomation": [],
          "effects": [
            {
              "id": "effect-isolation",
              "path": "__BUNDLE__",
              "state": {"stateData": null, "parameterValues": [], "bypassed": false}
            }
          ],
          "instrument": null,
          "audioClips": [],
          "midiClips": []
        }
      ]
    },
    "destination": "__DESTINATION__",
    "startTick": 0,
    "endTick": 960,
    "sampleRate": 48000,
    "blockSize": 512,
    "normalize": false
  }
}
]=])
string(REPLACE "__BUNDLE__" "${BUNDLE}" request "${request_template}")
string(REPLACE "__DESTINATION__" "${DESTINATION}" request "${request}")
# The worker reads exactly one line from standard input.
string(REGEX REPLACE "[\r\n]+" "" request "${request}")

set(request_file "${DESTINATION}.request.json")
file(WRITE "${request_file}" "${request}")

execute_process(
  COMMAND "${RENDER}"
  INPUT_FILE "${request_file}"
  OUTPUT_VARIABLE protocol_stdout
  ERROR_VARIABLE protocol_stderr
  RESULT_VARIABLE result
)
file(REMOVE "${request_file}")

if(NOT result EQUAL 0)
  message(FATAL_ERROR
    "riffra-render exited with ${result}\nstdout:\n${protocol_stdout}\nstderr:\n${protocol_stderr}")
endif()

string(STRIP "${protocol_stdout}" protocol_stdout)
string(FIND "${protocol_stdout}" "\n" line_break)
if(NOT line_break EQUAL -1)
  message(FATAL_ERROR "the protocol channel carried more than one line:\n${protocol_stdout}")
endif()

string(JSON message_type ERROR_VARIABLE json_error GET "${protocol_stdout}" type)
if(json_error)
  message(FATAL_ERROR
    "the protocol channel did not carry a JSON response (${json_error}):\n${protocol_stdout}")
endif()
if(NOT message_type STREQUAL "offlineRenderComplete")
  message(FATAL_ERROR "unexpected protocol response '${message_type}':\n${protocol_stdout}")
endif()

foreach(marker riffra-test-plugin-stdout-crt riffra-test-plugin-stdout-handle)
  string(FIND "${protocol_stderr}" "${marker}" position)
  if(position EQUAL -1)
    message(FATAL_ERROR "the plugin ${marker} output missed standard error:\n${protocol_stderr}")
  endif()
endforeach()

if(NOT EXISTS "${DESTINATION}")
  message(FATAL_ERROR "the rendered wave file was not written: ${DESTINATION}")
endif()
file(SIZE "${DESTINATION}" wave_size)
if(wave_size LESS 48)
  message(FATAL_ERROR "the rendered wave file is truncated (${wave_size} bytes)")
endif()
file(REMOVE "${DESTINATION}")

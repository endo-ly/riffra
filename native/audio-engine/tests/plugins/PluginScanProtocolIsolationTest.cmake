# Validates a VST3 that writes to the process standard output, and requires the
# scanner protocol to stay on standard output alone while the plugin output
# appears on standard error.
#
# Usage:
#   cmake -DSCAN=<riffra-plugin-scan> -DBUNDLE=<test vst3>
#         -P PluginScanProtocolIsolationTest.cmake

foreach(required SCAN BUNDLE)
  if(NOT DEFINED ${required})
    message(FATAL_ERROR "${required} is required")
  endif()
endforeach()

file(TO_CMAKE_PATH "${SCAN}" SCAN)
file(TO_CMAKE_PATH "${BUNDLE}" BUNDLE)

execute_process(
  COMMAND "${SCAN}" --validate-load "${BUNDLE}"
  OUTPUT_VARIABLE protocol_stdout
  ERROR_VARIABLE protocol_stderr
  RESULT_VARIABLE result
)

if(NOT result EQUAL 0)
  message(FATAL_ERROR
    "riffra-plugin-scan exited with ${result}\nstdout:\n${protocol_stdout}\nstderr:\n${protocol_stderr}")
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
if(NOT message_type STREQUAL "pluginLoadTestResult")
  message(FATAL_ERROR "unexpected protocol response '${message_type}':\n${protocol_stdout}")
endif()

foreach(marker riffra-test-plugin-stdout-crt riffra-test-plugin-stdout-handle)
  string(FIND "${protocol_stderr}" "${marker}" position)
  if(position EQUAL -1)
    message(FATAL_ERROR "the plugin ${marker} output missed standard error:\n${protocol_stderr}")
  endif()
endforeach()

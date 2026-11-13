file(WRITE "${REQUEST}" "{\"stateData\":null,\"parameterValues\":[0.25,0.7],\"displayRequests\":[{\"name\":\"Mode\",\"index\":2,\"display\":\"Warm\"}]}")
execute_process(
  COMMAND "${SCAN}" --describe-parameters "${BUNDLE}" "${REQUEST}"
  RESULT_VARIABLE result OUTPUT_VARIABLE output ERROR_VARIABLE error)
file(REMOVE "${REQUEST}")
if(NOT result EQUAL 0)
  message(FATAL_ERROR "parameter probe failed: ${result}; ${error}; ${output}")
endif()
string(JSON kind GET "${output}" type)
string(JSON gain GET "${output}" parameterValues 0)
string(JSON level GET "${output}" parameterValues 1)
string(JSON mode GET "${output}" conversions 0 0 value)
string(JSON name GET "${output}" parameters 2 name)
if(NOT kind STREQUAL "pluginParameters" OR NOT name STREQUAL "Mode"
   OR gain LESS 0.249 OR gain GREATER 0.251
   OR level LESS 0.699 OR level GREATER 0.701
   OR mode LESS 0.499 OR mode GREATER 0.501)
  message(FATAL_ERROR "parameter probe returned incorrect restored values or display conversion: ${output}")
endif()

#pragma once

#include <string_view>

namespace riffra {

/// Reserves the process standard output as the sidecar protocol channel.
///
/// The channel keeps writing to the standard output the process was started
/// with, and the process standard output itself is repointed at standard error
/// so that output written by hosted VST3 plugins cannot reach the protocol.
/// Call it once, before any plugin is loaded.
void isolateProtocolChannel();

/// Writes one protocol line to the sidecar protocol channel.
///
/// Writes to the process standard output until `isolateProtocolChannel()` has
/// reserved it.
void writeProtocolLine(std::string_view line);

}  // namespace riffra

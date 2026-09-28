#include "protocol/ProtocolChannel.h"

#include <cstdint>
#include <cstdio>

#if defined(_WIN32)
#include <fcntl.h>
#include <io.h>
#include <windows.h>
#else
#include <unistd.h>
#endif

namespace riffra {
namespace {

/// Reserved protocol stream; null until `isolateProtocolChannel()` runs.
FILE* protocolStream = nullptr;

#if defined(_WIN32)

FILE* reserveStandardOutput() {
    const HANDLE original = ::GetStdHandle(STD_OUTPUT_HANDLE);
    if (original == nullptr || original == INVALID_HANDLE_VALUE) return nullptr;
    HANDLE reserved = nullptr;
    if (!::DuplicateHandle(::GetCurrentProcess(), original, ::GetCurrentProcess(), &reserved, 0,
                           FALSE, DUPLICATE_SAME_ACCESS))
        return nullptr;
    const auto descriptor = static_cast<int>(
        ::_open_osfhandle(reinterpret_cast<std::intptr_t>(reserved), _O_WRONLY | _O_TEXT));
    if (descriptor < 0) {
        ::CloseHandle(reserved);
        return nullptr;
    }
    FILE* stream = ::_fdopen(descriptor, "w");
    if (stream == nullptr) ::_close(descriptor);
    return stream;
}

void divertStandardOutput() {
    std::fflush(stdout);
    ::SetStdHandle(STD_OUTPUT_HANDLE, ::GetStdHandle(STD_ERROR_HANDLE));
    ::_dup2(::_fileno(stderr), ::_fileno(stdout));
}

#else

FILE* reserveStandardOutput() {
    const int reserved = ::dup(STDOUT_FILENO);
    if (reserved < 0) return nullptr;
    FILE* stream = ::fdopen(reserved, "w");
    if (stream == nullptr) ::close(reserved);
    return stream;
}

void divertStandardOutput() {
    std::fflush(stdout);
    ::dup2(STDERR_FILENO, STDOUT_FILENO);
}

#endif

}  // namespace

void isolateProtocolChannel() {
    FILE* reserved = reserveStandardOutput();
    if (reserved == nullptr) return;
    divertStandardOutput();
    protocolStream = reserved;
}

void writeProtocolLine(const std::string_view line) {
    FILE* stream = protocolStream != nullptr ? protocolStream : stdout;
    std::fwrite(line.data(), 1, line.size(), stream);
    std::fputc('\n', stream);
    std::fflush(stream);
}

}  // namespace riffra

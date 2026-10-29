#include "process_activity.hpp"

#include <windows.h>

#include <charconv>
#include <cstdint>
#include <string>

namespace aura_main_pipe {
namespace {

uint64_t Combined(const FILETIME& value) {
  ULARGE_INTEGER combined{};
  combined.LowPart = value.dwLowDateTime;
  combined.HighPart = value.dwHighDateTime;
  return combined.QuadPart;
}

bool ReadArguments(napi_env env, napi_callback_info info, DWORD* pid,
                   uint64_t* creation_time) {
  size_t argc = 2;
  napi_value argv[2]{};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc != 2) {
    return false;
  }
  uint32_t process_id = 0;
  if (napi_get_value_uint32(env, argv[0], &process_id) != napi_ok ||
      process_id == 0) {
    return false;
  }
  char text[32]{};
  size_t length = 0;
  if (napi_get_value_string_utf8(env, argv[1], text, sizeof(text), &length) !=
          napi_ok ||
      length == 0 || length >= sizeof(text) - 1) {
    return false;
  }
  const auto parsed = std::from_chars(text, text + length, *creation_time);
  if (parsed.ec != std::errc{} || parsed.ptr != text + length) {
    return false;
  }
  *pid = process_id;
  return true;
}

napi_value Decimal(napi_env env, uint64_t value) {
  const std::string text = std::to_string(value);
  napi_value result = nullptr;
  napi_create_string_utf8(env, text.c_str(), text.size(), &result);
  return result;
}

}  // namespace

napi_value ProcessActivity(napi_env env, napi_callback_info info) {
  DWORD pid = 0;
  uint64_t expected_creation_time = 0;
  if (!ReadArguments(env, info, &pid, &expected_creation_time)) {
    napi_throw_error(env, "ERR_AURA_PIPE_ARGUMENT",
                     "processActivity takes a process id and its creation time");
    return nullptr;
  }
  napi_value null_value = nullptr;
  napi_get_null(env, &null_value);
  HANDLE process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
  if (process == nullptr) {
    return null_value;
  }
  FILETIME created{};
  FILETIME exited{};
  FILETIME kernel{};
  FILETIME user{};
  IO_COUNTERS io{};
  const bool read =
      GetProcessTimes(process, &created, &exited, &kernel, &user) != FALSE &&
      Combined(created) == expected_creation_time &&
      GetProcessIoCounters(process, &io) != FALSE;
  CloseHandle(process);
  if (!read) {
    return null_value;
  }
  napi_value result = nullptr;
  napi_create_object(env, &result);
  napi_set_named_property(env, result, "cpuTime",
                          Decimal(env, Combined(kernel) + Combined(user)));
  napi_set_named_property(
      env, result, "ioOperations",
      Decimal(env, io.ReadOperationCount + io.WriteOperationCount));
  return result;
}

}  // namespace aura_main_pipe

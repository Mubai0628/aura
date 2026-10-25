#pragma once
#include <node_api.h>
#include <windows.h>
#include <string>
namespace aura_main_pipe {
inline napi_value CreateError(napi_env env, const char* code,
                       const std::string& message, DWORD win32_error) {
  napi_value message_value = nullptr;
  napi_value code_value = nullptr;
  napi_value error = nullptr;
  napi_value win32_value = nullptr;
  napi_create_string_utf8(env, message.c_str(), message.size(), &message_value);
  napi_create_string_utf8(env, code, NAPI_AUTO_LENGTH, &code_value);
  napi_create_error(env, code_value, message_value, &error);
  napi_create_uint32(env, win32_error, &win32_value);
  napi_set_named_property(env, error, "win32Error", win32_value);
  return error;
}

inline void Reject(napi_env env, napi_deferred deferred, const char* code,
            const std::string& message, DWORD win32_error) {
  napi_reject_deferred(env, deferred,
                       CreateError(env, code, message, win32_error));
}

}

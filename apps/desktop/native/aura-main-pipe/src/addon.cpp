#define NAPI_VERSION 10
#define _WIN32_WINNT 0x0A00

#include "connection_io.hpp"
#include "napi_errors.hpp"
#include <node_api.h>
#include <windows.h>

#include <algorithm>
#include <atomic>
#include <charconv>
#include <cstdint>
#include <memory>
#include <mutex>
#include <string>
#include <thread>
#include <utility>
#include <vector>

namespace {
using namespace aura_main_pipe;

constexpr uint32_t kBridgeAbiVersion = 1;
constexpr uint32_t kProtocolMajor = 1;
constexpr uint32_t kProtocolMinor = 7;
constexpr DWORD kConnectTimeoutMs = 10'000;
constexpr DWORD kRetryDelayMs = 25;
constexpr size_t kMaximumReadBytes = 1024 * 1024;

struct AddonState {
  napi_ref connection_constructor = nullptr;
};

struct ConnectWork {
  napi_env env = nullptr;
  napi_deferred deferred = nullptr;
  napi_async_work work = nullptr;
  std::string namespace_object_hash;
  std::string runtime_instance_id;
  std::string pipe_nonce;
  DWORD expected_server_pid = 0;
  bool has_expected_server_creation_time = false;
  uint64_t expected_server_creation_time = 0;
  bool has_expected_server_windows_session_id = false;
  DWORD expected_server_windows_session_id = 0;
  HANDLE pipe = INVALID_HANDLE_VALUE;
  HANDLE completion_port = nullptr;
  HANDLE server_process = nullptr;
  uint64_t server_creation_time = 0;
  DWORD server_windows_session_id = 0;
  DWORD error = ERROR_SUCCESS;
  std::string error_code;
  std::string error_message;
};

uint64_t FileTimeValue(const FILETIME& value) {
  ULARGE_INTEGER combined{};
  combined.LowPart = value.dwLowDateTime;
  combined.HighPart = value.dwHighDateTime;
  return combined.QuadPart;
}

bool IsCrockford(char value) {
  return (value >= '0' && value <= '9') ||
         (value >= 'A' && value <= 'Z' && value != 'I' && value != 'L' &&
          value != 'O' && value != 'U');
}

bool IsRuntimeInstanceId(const std::string& value) {
  if (value.size() != 30 || value.rfind("run_", 0) != 0) {
    return false;
  }
  return std::all_of(value.begin() + 4, value.end(), IsCrockford);
}

bool IsPipeNonce(const std::string& value) {
  return value.size() == 32 &&
         std::all_of(value.begin(), value.end(), [](char character) {
           return (character >= '0' && character <= '9') ||
                  (character >= 'a' && character <= 'f');
         });
}

bool IsNamespaceObjectHash(const std::string& value) {
  return IsPipeNonce(value);
}

std::wstring BuildPipeName(const std::string& namespace_object_hash,
                           const std::string& runtime_instance_id,
                           const std::string& pipe_nonce) {
  const std::string ascii = "\\\\.\\pipe\\Aura\\" + namespace_object_hash +
                            "\\" + runtime_instance_id + "\\" + pipe_nonce;
  return std::wstring(ascii.begin(), ascii.end());
}

void SetConnectError(ConnectWork* work, DWORD error, std::string code,
                     std::string message) {
  work->error = error;
  work->error_code = std::move(code);
  work->error_message = std::move(message);
}

bool SnapshotProcessIdentity(DWORD process_id, HANDLE* process,
                             uint64_t* creation_time,
                             DWORD* windows_session_id) {
  HANDLE handle =
      OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, FALSE,
                  process_id);
  if (handle == nullptr) {
    return false;
  }
  FILETIME created{};
  FILETIME exited{};
  FILETIME kernel{};
  FILETIME user{};
  if (GetProcessTimes(handle, &created, &exited, &kernel, &user) == FALSE) {
    CloseHandle(handle);
    return false;
  }
  DWORD session_id = 0;
  if (ProcessIdToSessionId(process_id, &session_id) == FALSE) {
    CloseHandle(handle);
    return false;
  }
  *process = handle;
  *creation_time = FileTimeValue(created);
  *windows_session_id = session_id;
  return true;
}

bool OpenAndVerifyPipe(ConnectWork* work) {
  HANDLE expected_process = nullptr;
  uint64_t expected_creation_time = 0;
  DWORD expected_windows_session_id = 0;
  if (!SnapshotProcessIdentity(work->expected_server_pid, &expected_process,
                               &expected_creation_time,
                               &expected_windows_session_id)) {
    SetConnectError(work, GetLastError(), "ERR_AURA_PIPE_PROCESS_IDENTITY",
                    "failed to snapshot the expected Aura Core process");
    return false;
  }
  if ((work->has_expected_server_creation_time &&
       work->expected_server_creation_time != expected_creation_time) ||
      (work->has_expected_server_windows_session_id &&
       work->expected_server_windows_session_id !=
           expected_windows_session_id)) {
    CloseHandle(expected_process);
    SetConnectError(work, ERROR_ACCESS_DENIED,
                    "ERR_AURA_PIPE_PROCESS_IDENTITY",
                    "Aura Core process object changed before reconnect");
    return false;
  }

  const std::wstring pipe_name =
      BuildPipeName(work->namespace_object_hash, work->runtime_instance_id,
                    work->pipe_nonce);
  const ULONGLONG deadline = GetTickCount64() + kConnectTimeoutMs;
  HANDLE pipe = INVALID_HANDLE_VALUE;
  while (GetTickCount64() < deadline) {
    pipe = CreateFileW(
        pipe_name.c_str(), FILE_READ_DATA | FILE_WRITE_DATA | SYNCHRONIZE, 0,
        nullptr, OPEN_EXISTING,
        FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
        nullptr);
    if (pipe != INVALID_HANDLE_VALUE) {
      break;
    }
    const DWORD error = GetLastError();
    if (error != ERROR_FILE_NOT_FOUND && error != ERROR_PIPE_BUSY) {
      CloseHandle(expected_process);
      SetConnectError(work, error, "ERR_AURA_PIPE_CONNECT",
                      "Aura Core pipe connection was rejected");
      return false;
    }
    if (WaitForSingleObject(expected_process, 0) != WAIT_TIMEOUT) {
      CloseHandle(expected_process);
      SetConnectError(work, ERROR_PROCESS_ABORTED,
                      "ERR_AURA_PIPE_PROCESS_IDENTITY",
                      "Aura Core exited before the pipe was authenticated");
      return false;
    }
    WaitNamedPipeW(pipe_name.c_str(), kRetryDelayMs);
    Sleep(kRetryDelayMs);
  }
  if (pipe == INVALID_HANDLE_VALUE) {
    const DWORD error = GetLastError();
    CloseHandle(expected_process);
    SetConnectError(work, error == ERROR_SUCCESS ? ERROR_SEM_TIMEOUT : error,
                    "ERR_AURA_PIPE_CONNECT_TIMEOUT",
                    "timed out connecting to the Aura Core pipe");
    return false;
  }

  DWORD server_pid = 0;
  if (GetNamedPipeServerProcessId(pipe, &server_pid) == FALSE ||
      server_pid != work->expected_server_pid) {
    const DWORD error =
        server_pid == work->expected_server_pid ? GetLastError()
                                                : ERROR_ACCESS_DENIED;
    CloseHandle(pipe);
    CloseHandle(expected_process);
    SetConnectError(work, error, "ERR_AURA_PIPE_PROCESS_IDENTITY",
                    "named-pipe server PID did not match Aura Core");
    return false;
  }

  HANDLE actual_process = nullptr;
  uint64_t actual_creation_time = 0;
  DWORD actual_windows_session_id = 0;
  if (!SnapshotProcessIdentity(server_pid, &actual_process,
                               &actual_creation_time,
                               &actual_windows_session_id) ||
      actual_creation_time != expected_creation_time ||
      actual_windows_session_id != expected_windows_session_id ||
      WaitForSingleObject(expected_process, 0) != WAIT_TIMEOUT) {
    const DWORD error = GetLastError();
    if (actual_process != nullptr) {
      CloseHandle(actual_process);
    }
    CloseHandle(pipe);
    CloseHandle(expected_process);
    SetConnectError(work, error == ERROR_SUCCESS ? ERROR_ACCESS_DENIED : error,
                    "ERR_AURA_PIPE_PROCESS_IDENTITY",
                    "named-pipe server creation time did not match Aura Core");
    return false;
  }
  CloseHandle(actual_process);

  HANDLE completion_port =
      CreateIoCompletionPort(pipe, nullptr, 0, 1);
  if (completion_port == nullptr) {
    const DWORD error = GetLastError();
    CloseHandle(pipe);
    CloseHandle(expected_process);
    SetConnectError(work, error, "ERR_AURA_PIPE_IOCP",
                    "failed to attach Aura Core pipe to an IO completion port");
    return false;
  }

  work->pipe = pipe;
  work->completion_port = completion_port;
  work->server_process = expected_process;
  work->server_creation_time = actual_creation_time;
  work->server_windows_session_id = actual_windows_session_id;
  return true;
}

bool GetNamedString(napi_env env, napi_value object, const char* name,
                    std::string* output) {
  napi_value value = nullptr;
  if (napi_get_named_property(env, object, name, &value) != napi_ok) {
    return false;
  }
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok || type != napi_string) {
    return false;
  }
  size_t length = 0;
  if (napi_get_value_string_utf8(env, value, nullptr, 0, &length) != napi_ok) {
    return false;
  }
  std::vector<char> bytes(length + 1);
  if (napi_get_value_string_utf8(env, value, bytes.data(), bytes.size(),
                                 &length) != napi_ok) {
    return false;
  }
  output->assign(bytes.data(), length);
  return true;
}

bool GetNamedUint32(napi_env env, napi_value object, const char* name,
                    uint32_t* output) {
  napi_value value = nullptr;
  return napi_get_named_property(env, object, name, &value) == napi_ok &&
         napi_get_value_uint32(env, value, output) == napi_ok;
}

bool GetOptionalNamedString(napi_env env, napi_value object, const char* name,
                            bool* present, std::string* output) {
  napi_value value = nullptr;
  if (napi_get_named_property(env, object, name, &value) != napi_ok) {
    return false;
  }
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok) {
    return false;
  }
  if (type == napi_undefined || type == napi_null) {
    *present = false;
    return true;
  }
  *present = true;
  return GetNamedString(env, object, name, output);
}

bool GetOptionalNamedUint32(napi_env env, napi_value object, const char* name,
                            bool* present, uint32_t* output) {
  napi_value value = nullptr;
  if (napi_get_named_property(env, object, name, &value) != napi_ok) {
    return false;
  }
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok) {
    return false;
  }
  if (type == napi_undefined || type == napi_null) {
    *present = false;
    return true;
  }
  *present = true;
  return napi_get_value_uint32(env, value, output) == napi_ok;
}

bool ParseUint64(const std::string& text, uint64_t* value) {
  if (text.empty()) {
    return false;
  }
  const char* first = text.data();
  const char* last = first + text.size();
  const auto result = std::from_chars(first, last, *value);
  return result.ec == std::errc{} && result.ptr == last;
}

napi_value ConnectionConstructor(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1]{};
  napi_value this_value = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_value, nullptr);
  if (argc != 1) {
    napi_throw_type_error(env, "ERR_AURA_PIPE_INTERNAL",
                          "native Connection cannot be constructed directly");
    return nullptr;
  }
  void* external = nullptr;
  if (napi_get_value_external(env, argv[0], &external) != napi_ok ||
      external == nullptr) {
    napi_throw_type_error(env, "ERR_AURA_PIPE_INTERNAL",
                          "invalid native Connection payload");
    return nullptr;
  }
  auto* connection = static_cast<Connection*>(external);
  if (napi_wrap(env, this_value, connection, FinalizeConnection, nullptr,
                &connection->wrapper) != napi_ok) {
    napi_throw_error(env, "ERR_AURA_PIPE_INTERNAL",
                     "failed to wrap native Connection");
    return nullptr;
  }
  return this_value;
}

Connection* UnwrapConnection(napi_env env, napi_callback_info info,
                             size_t* argc, napi_value* argv,
                             napi_value* this_value) {
  if (napi_get_cb_info(env, info, argc, argv, this_value, nullptr) != napi_ok) {
    return nullptr;
  }
  Connection* connection = nullptr;
  if (napi_unwrap(env, *this_value,
                  reinterpret_cast<void**>(&connection)) != napi_ok) {
    return nullptr;
  }
  return connection;
}

napi_value CreateRejectedPromise(napi_env env, const char* code,
                                 const std::string& message,
                                 DWORD win32_error) {
  napi_deferred deferred = nullptr;
  napi_value promise = nullptr;
  napi_create_promise(env, &deferred, &promise);
  Reject(env, deferred, code, message, win32_error);
  return promise;
}

napi_value Read(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1]{};
  napi_value this_value = nullptr;
  Connection* connection =
      UnwrapConnection(env, info, &argc, argv, &this_value);
  if (connection == nullptr || argc != 1) {
    return CreateRejectedPromise(env, "ERR_AURA_PIPE_ARGUMENT",
                                 "read requires a byte limit", ERROR_INVALID_PARAMETER);
  }
  uint32_t maximum = 0;
  if (napi_get_value_uint32(env, argv[0], &maximum) != napi_ok ||
      maximum == 0 || maximum > kMaximumReadBytes) {
    return CreateRejectedPromise(env, "ERR_AURA_PIPE_ARGUMENT",
                                 "read byte limit is invalid",
                                 ERROR_INVALID_PARAMETER);
  }
  if (connection->closing.load() ||
      connection->read_pending.exchange(true)) {
    return CreateRejectedPromise(
        env, "ERR_AURA_PIPE_STATE",
        connection->closing.load() ? "Aura Core pipe is closed"
                                   : "a pipe read is already pending",
        ERROR_INVALID_STATE);
  }

  auto operation = std::make_unique<Operation>();
  operation->connection = connection;
  operation->kind = OperationKind::Read;
  operation->bytes.resize(maximum);
  napi_value promise = nullptr;
  napi_create_promise(env, &operation->deferred, &promise);
  if (!BeginOperation(connection)) {
    connection->read_pending.store(false);
    Reject(env, operation->deferred, "ERR_AURA_PIPE_INTERNAL",
           "failed to retain native pipe connection", ERROR_INVALID_STATE);
    return promise;
  }

  {
    std::lock_guard<std::mutex> lock(connection->io_mutex);
    if (connection->pipe == INVALID_HANDLE_VALUE) {
      operation->error = ERROR_INVALID_HANDLE;
    } else {
      const BOOL submitted =
          ReadFile(connection->pipe, operation->bytes.data(), maximum, nullptr,
                   &operation->overlapped);
      if (submitted == FALSE) {
        const DWORD error = GetLastError();
        if (error != ERROR_IO_PENDING) {
          operation->error = error;
        }
      }
    }
  }
  if (operation->error != ERROR_SUCCESS) {
    QueueSyntheticCompletion(operation.release());
  } else {
    operation.release();
  }
  return promise;
}

napi_value Write(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1]{};
  napi_value this_value = nullptr;
  Connection* connection =
      UnwrapConnection(env, info, &argc, argv, &this_value);
  if (connection == nullptr || argc != 1) {
    return CreateRejectedPromise(env, "ERR_AURA_PIPE_ARGUMENT",
                                 "write requires a Buffer", ERROR_INVALID_PARAMETER);
  }
  bool is_buffer = false;
  void* data = nullptr;
  size_t length = 0;
  if (napi_is_buffer(env, argv[0], &is_buffer) != napi_ok || !is_buffer ||
      napi_get_buffer_info(env, argv[0], &data, &length) != napi_ok ||
      length == 0 || length > UINT32_MAX) {
    return CreateRejectedPromise(env, "ERR_AURA_PIPE_ARGUMENT",
                                 "write Buffer is invalid",
                                 ERROR_INVALID_PARAMETER);
  }
  if (connection->closing.load() ||
      connection->write_pending.exchange(true)) {
    return CreateRejectedPromise(
        env, "ERR_AURA_PIPE_STATE",
        connection->closing.load() ? "Aura Core pipe is closed"
                                   : "a pipe write is already pending",
        ERROR_INVALID_STATE);
  }

  auto operation = std::make_unique<Operation>();
  operation->connection = connection;
  operation->kind = OperationKind::Write;
  const auto* first = static_cast<const uint8_t*>(data);
  operation->bytes.assign(first, first + length);
  napi_value promise = nullptr;
  napi_create_promise(env, &operation->deferred, &promise);
  if (!BeginOperation(connection)) {
    connection->write_pending.store(false);
    Reject(env, operation->deferred, "ERR_AURA_PIPE_INTERNAL",
           "failed to retain native pipe connection", ERROR_INVALID_STATE);
    return promise;
  }
  if (!SubmitWriteRemainder(operation.get())) {
    QueueSyntheticCompletion(operation.release());
  } else {
    operation.release();
  }
  return promise;
}

napi_value Cancel(napi_env env, napi_callback_info info) {
  size_t argc = 0;
  napi_value this_value = nullptr;
  Connection* connection =
      UnwrapConnection(env, info, &argc, nullptr, &this_value);
  bool cancelled = false;
  if (connection != nullptr) {
    std::lock_guard<std::mutex> lock(connection->io_mutex);
    if (connection->pipe != INVALID_HANDLE_VALUE) {
      cancelled = CancelIoEx(connection->pipe, nullptr) != FALSE ||
                  GetLastError() == ERROR_NOT_FOUND;
    }
  }
  napi_value result = nullptr;
  napi_get_boolean(env, cancelled, &result);
  return result;
}

napi_value Close(napi_env env, napi_callback_info info) {
  size_t argc = 0;
  napi_value this_value = nullptr;
  Connection* connection =
      UnwrapConnection(env, info, &argc, nullptr, &this_value);
  if (connection != nullptr) {
    RequestConnectionClose(connection);
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ConnectionInfo(napi_env env, napi_callback_info info) {
  size_t argc = 0;
  napi_value this_value = nullptr;
  Connection* connection =
      UnwrapConnection(env, info, &argc, nullptr, &this_value);
  if (connection == nullptr) {
    napi_throw_error(env, "ERR_AURA_PIPE_STATE",
                     "native pipe connection is unavailable");
    return nullptr;
  }
  napi_value result = nullptr;
  napi_value pid = nullptr;
  napi_value creation = nullptr;
  napi_value session = nullptr;
  napi_create_object(env, &result);
  napi_create_uint32(env, connection->server_pid, &pid);
  const std::string creation_text =
      std::to_string(connection->server_creation_time);
  napi_create_string_utf8(env, creation_text.c_str(), creation_text.size(),
                          &creation);
  napi_create_uint32(env, connection->server_windows_session_id, &session);
  napi_set_named_property(env, result, "serverPid", pid);
  napi_set_named_property(env, result, "serverCreationTime", creation);
  napi_set_named_property(env, result, "serverWindowsSessionId", session);
  return result;
}

void ConnectExecute(napi_env, void* data) {
  auto* work = static_cast<ConnectWork*>(data);
  if (!IsNamespaceObjectHash(work->namespace_object_hash) ||
      !IsRuntimeInstanceId(work->runtime_instance_id) ||
      !IsPipeNonce(work->pipe_nonce) || work->expected_server_pid == 0) {
    SetConnectError(work, ERROR_INVALID_PARAMETER, "ERR_AURA_PIPE_ARGUMENT",
                    "Aura Core pipe identity arguments are invalid");
    return;
  }
  OpenAndVerifyPipe(work);
}

void ConnectComplete(napi_env env, napi_status status, void* data) {
  std::unique_ptr<ConnectWork> work(static_cast<ConnectWork*>(data));
  napi_delete_async_work(env, work->work);
  if (status != napi_ok || work->pipe == INVALID_HANDLE_VALUE ||
      work->completion_port == nullptr || work->server_process == nullptr) {
    if (work->server_process != nullptr) {
      CloseHandle(work->server_process);
      work->server_process = nullptr;
    }
    Reject(env, work->deferred,
           work->error_code.empty() ? "ERR_AURA_PIPE_CONNECT"
                                    : work->error_code.c_str(),
           work->error_message.empty() ? "Aura Core pipe connection failed"
                                       : work->error_message,
           work->error);
    return;
  }

  auto connection = std::make_unique<Connection>();
  connection->env = env;
  connection->pipe = work->pipe;
  connection->completion_port = work->completion_port;
  connection->server_process = work->server_process;
  connection->server_pid = work->expected_server_pid;
  connection->server_creation_time = work->server_creation_time;
  connection->server_windows_session_id = work->server_windows_session_id;
  work->pipe = INVALID_HANDLE_VALUE;
  work->completion_port = nullptr;
  work->server_process = nullptr;

  AddonState* state = nullptr;
  napi_get_instance_data(env, reinterpret_cast<void**>(&state));
  napi_value constructor = nullptr;
  napi_value external = nullptr;
  napi_value instance = nullptr;
  if (state == nullptr ||
      napi_get_reference_value(env, state->connection_constructor,
                               &constructor) != napi_ok ||
      napi_create_external(env, connection.get(), nullptr, nullptr,
                           &external) != napi_ok ||
      !StartConnection(connection.get()) ||
      napi_new_instance(env, constructor, 1, &external, &instance) != napi_ok) {
    FinalizeConnection(env, connection.release(), nullptr);
    Reject(env, work->deferred, "ERR_AURA_PIPE_INTERNAL",
           "failed to initialize the native pipe connection",
           ERROR_INVALID_STATE);
    return;
  }

  connection.release();
  napi_resolve_deferred(env, work->deferred, instance);
}

napi_value Connect(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1]{};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  napi_value promise = nullptr;
  napi_deferred deferred = nullptr;
  napi_create_promise(env, &deferred, &promise);
  napi_valuetype type = napi_undefined;
  if (argc != 1 || napi_typeof(env, argv[0], &type) != napi_ok ||
      type != napi_object) {
    Reject(env, deferred, "ERR_AURA_PIPE_ARGUMENT",
           "connect requires a frozen Aura Core endpoint",
           ERROR_INVALID_PARAMETER);
    return promise;
  }

  auto work = std::make_unique<ConnectWork>();
  work->env = env;
  work->deferred = deferred;
  uint32_t expected_pid = 0;
  std::string expected_creation_time;
  uint32_t expected_windows_session_id = 0;
  if (!GetNamedString(env, argv[0], "namespaceObjectHash",
                      &work->namespace_object_hash) ||
      !GetNamedString(env, argv[0], "runtimeInstanceId",
                      &work->runtime_instance_id) ||
      !GetNamedString(env, argv[0], "pipeNonce", &work->pipe_nonce) ||
      !GetNamedUint32(env, argv[0], "expectedServerPid", &expected_pid) ||
      !GetOptionalNamedString(
          env, argv[0], "expectedServerCreationTime",
          &work->has_expected_server_creation_time,
          &expected_creation_time) ||
      !GetOptionalNamedUint32(
          env, argv[0], "expectedServerWindowsSessionId",
          &work->has_expected_server_windows_session_id,
          &expected_windows_session_id) ||
      (work->has_expected_server_creation_time &&
       !ParseUint64(expected_creation_time,
                    &work->expected_server_creation_time))) {
    Reject(env, deferred, "ERR_AURA_PIPE_ARGUMENT",
           "connect endpoint is missing a required identity field",
           ERROR_INVALID_PARAMETER);
    return promise;
  }
  work->expected_server_pid = expected_pid;
  work->expected_server_windows_session_id = expected_windows_session_id;
  napi_value resource_name = nullptr;
  napi_create_string_utf8(env, "aura-main-pipe-connect", NAPI_AUTO_LENGTH,
                          &resource_name);
  if (napi_create_async_work(env, nullptr, resource_name, ConnectExecute,
                             ConnectComplete, work.get(),
                             &work->work) != napi_ok ||
      napi_queue_async_work(env, work->work) != napi_ok) {
    Reject(env, deferred, "ERR_AURA_PIPE_INTERNAL",
           "failed to queue Aura Core pipe connection", ERROR_INVALID_STATE);
    return promise;
  }
  work.release();
  return promise;
}

napi_value BridgeInfo(napi_env env, napi_callback_info) {
  napi_value result = nullptr;
  napi_value value = nullptr;
  napi_create_object(env, &result);
  napi_create_uint32(env, kBridgeAbiVersion, &value);
  napi_set_named_property(env, result, "bridgeAbiVersion", value);
  napi_create_uint32(env, NAPI_VERSION, &value);
  napi_set_named_property(env, result, "napiVersion", value);
  napi_create_string_utf8(env, "win32-x64", NAPI_AUTO_LENGTH, &value);
  napi_set_named_property(env, result, "target", value);
  napi_create_uint32(env, kProtocolMajor, &value);
  napi_set_named_property(env, result, "protocolMajor", value);
  napi_create_uint32(env, kProtocolMinor, &value);
  napi_set_named_property(env, result, "protocolMinor", value);
  return result;
}

void FinalizeAddonState(napi_env env, void* data, void*) {
  auto* state = static_cast<AddonState*>(data);
  if (state->connection_constructor != nullptr) {
    napi_delete_reference(env, state->connection_constructor);
  }
  delete state;
}

napi_value Initialize(napi_env env, napi_value exports) {
#if !defined(_WIN64) || !defined(AURA_MAIN_PIPE_WIN32_X64)
#error aura-main-pipe supports win32-x64 only
#endif
  auto state = std::make_unique<AddonState>();
  napi_property_descriptor methods[] = {
      {"read", nullptr, Read, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"write", nullptr, Write, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"cancel", nullptr, Cancel, nullptr, nullptr, nullptr, napi_default,
       nullptr},
      {"close", nullptr, Close, nullptr, nullptr, nullptr, napi_default,
       nullptr},
      {"info", nullptr, ConnectionInfo, nullptr, nullptr, nullptr, napi_default,
       nullptr},
  };
  napi_value constructor = nullptr;
  if (napi_define_class(env, "AuraMainPipeConnection", NAPI_AUTO_LENGTH,
                        ConnectionConstructor, nullptr,
                        sizeof(methods) / sizeof(methods[0]), methods,
                        &constructor) != napi_ok ||
      napi_create_reference(env, constructor, 1,
                            &state->connection_constructor) != napi_ok) {
    napi_throw_error(env, "ERR_AURA_PIPE_INTERNAL",
                     "failed to initialize Aura Main Pipe class");
    return nullptr;
  }

  napi_property_descriptor exports_descriptors[] = {
      {"connect", nullptr, Connect, nullptr, nullptr, nullptr, napi_default,
       nullptr},
      {"getBridgeInfo", nullptr, BridgeInfo, nullptr, nullptr, nullptr,
       napi_default, nullptr},
  };
  if (napi_define_properties(
          env, exports,
          sizeof(exports_descriptors) / sizeof(exports_descriptors[0]),
          exports_descriptors) != napi_ok ||
      napi_set_instance_data(env, state.get(), FinalizeAddonState, nullptr) !=
          napi_ok) {
    napi_throw_error(env, "ERR_AURA_PIPE_INTERNAL",
                     "failed to export Aura Main Pipe bridge");
    return nullptr;
  }
  state.release();
  return exports;
}

}  // namespace

NAPI_MODULE(NODE_GYP_MODULE_NAME, Initialize)

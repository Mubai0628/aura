#pragma once
#include <node_api.h>
#include <windows.h>
#include <atomic>
#include <cstdint>
#include <mutex>
#include <thread>
#include <vector>

namespace aura_main_pipe {
struct Connection;
enum class OperationKind { Read, Write };
struct Operation {
  OVERLAPPED overlapped{};
  Connection* connection = nullptr;
  OperationKind kind = OperationKind::Read;
  napi_deferred deferred = nullptr;
  std::vector<uint8_t> bytes;
  size_t offset = 0;
  DWORD transferred = 0;
  DWORD error = ERROR_SUCCESS;
};
struct Connection {
  napi_env env = nullptr;
  HANDLE pipe = INVALID_HANDLE_VALUE;
  HANDLE completion_port = nullptr;
  HANDLE server_process = nullptr;
  DWORD server_pid = 0;
  uint64_t server_creation_time = 0;
  DWORD server_windows_session_id = 0;
  napi_threadsafe_function completions = nullptr;
  napi_ref wrapper = nullptr;
  std::thread completion_thread;
  std::mutex io_mutex;
  std::atomic<bool> closing{false};
  std::atomic<bool> environment_closing{false};
  std::atomic<bool> read_pending{false};
  std::atomic<bool> write_pending{false};
  // Kernel completion must never wait for the JS callback that retains a wrapper.
  std::atomic<uint32_t> pending_native_io{0};
  std::atomic<uint32_t> pending_operations{0};
  std::atomic<bool> wrapper_retained{false};
  bool cleanup_hook_registered = false;
  // Wrapper and TSFN own separate lifetimes; abort callbacks can arrive after wrap cleanup.
  std::atomic<uint32_t> owners{1};
  bool stop_posted = false;
};
bool StartConnection(Connection* connection);
void RequestConnectionClose(Connection* connection);
void FinalizeConnection(napi_env env, void* data, void* hint);
bool BeginOperation(Connection* connection);
bool SubmitWriteRemainder(Operation* operation);
void QueueSyntheticCompletion(Operation* operation);
}  // namespace aura_main_pipe

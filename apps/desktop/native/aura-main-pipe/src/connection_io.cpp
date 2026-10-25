#include "connection_io.hpp"
#include "napi_errors.hpp"
#include <algorithm>
#include <memory>

namespace aura_main_pipe {
namespace {
constexpr ULONG_PTR kShutdownCompletionKey = 1;
void ReleaseOwner(Connection* connection) {
  if (connection->owners.fetch_sub(1) == 1) delete connection;
}
void CloseForEnvironment(void* data) {
  auto* connection = static_cast<Connection*>(data);
  connection->cleanup_hook_registered = false;
  connection->environment_closing.store(true);
  // Environment cleanup waits for active TSFNs before wrap finalization. Cancel
  // here, while only kernel completion is needed to release that TSFN.
  RequestConnectionClose(connection);
  if (connection->completion_thread.joinable()) connection->completion_thread.join();
  ReleaseOwner(connection);
}
void ReleaseWrapperRetention(napi_env env, Connection* connection) {
  if (env != nullptr && connection->wrapper != nullptr &&
      connection->wrapper_retained.exchange(false)) {
    uint32_t ignored = 0;
    napi_reference_unref(env, connection->wrapper, &ignored);
  }
}
void FinalizeCompletions(napi_env env, void* data, void*) {
  auto* connection = static_cast<Connection*>(data);
  ReleaseWrapperRetention(env, connection);
  ReleaseOwner(connection);
}
void StopNativeConnection(Connection* connection) {
  std::lock_guard<std::mutex> lock(connection->io_mutex);
  if (connection->stop_posted) return;
  connection->stop_posted = true;
  if (connection->pipe != INVALID_HANDLE_VALUE) {
    CloseHandle(connection->pipe);
    connection->pipe = INVALID_HANDLE_VALUE;
  }
  if (connection->completion_port != nullptr) {
    PostQueuedCompletionStatus(connection->completion_port, 0,
                               kShutdownCompletionKey, nullptr);
  }
}
void ReleaseOperation(Operation* data, napi_env env) {
  std::unique_ptr<Operation> operation(data);
  Connection* connection = operation->connection;
  if (operation->kind == OperationKind::Read) connection->read_pending.store(false);
  else connection->write_pending.store(false);
  if (connection->pending_operations.fetch_sub(1) == 1) {
    ReleaseWrapperRetention(env, connection);
  }
}
void FinishOperationOnMain(napi_env env, napi_value, void*, void* data) {
  auto* operation = static_cast<Operation*>(data);
  // On abort Node passes env=nullptr. Still free exactly once, without N-API calls.
  if (env != nullptr) {
    if (operation->error != ERROR_SUCCESS) {
      Reject(env, operation->deferred,
             operation->error == ERROR_OPERATION_ABORTED
                 ? "ERR_AURA_PIPE_CANCELLED" : "ERR_AURA_PIPE_IO",
             "Aura Core pipe I/O failed", operation->error);
    } else if (operation->kind == OperationKind::Read) {
      napi_value buffer = nullptr;
      void* copied = nullptr;
      napi_create_buffer_copy(env, operation->transferred,
                              operation->bytes.data(), &copied, &buffer);
      napi_resolve_deferred(env, operation->deferred, buffer);
    } else {
      napi_value undefined = nullptr;
      napi_get_undefined(env, &undefined);
      napi_resolve_deferred(env, operation->deferred, undefined);
    }
  }
  ReleaseOperation(operation, env);
}
void CompleteNativeOperation(Operation* operation) {
  Connection* connection = operation->connection;
  if (connection->pending_native_io.fetch_sub(1) == 1 && connection->closing.load()) {
    StopNativeConnection(connection);
  }
  // At most one read and one write are retained. Never block a completion on JS.
  if (napi_call_threadsafe_function(connection->completions, operation,
                                    napi_tsfn_nonblocking) != napi_ok) {
    ReleaseOperation(operation, nullptr);
  }
}
void CompletionLoop(Connection* connection) {
  for (;;) {
    DWORD transferred = 0;
    ULONG_PTR key = 0;
    OVERLAPPED* overlapped = nullptr;
    const BOOL success = GetQueuedCompletionStatus(connection->completion_port,
                                                   &transferred, &key, &overlapped, INFINITE);
    if (overlapped == nullptr && key == kShutdownCompletionKey) break;
    if (overlapped == nullptr) continue;
    auto* operation = CONTAINING_RECORD(overlapped, Operation, overlapped);
    operation->error = success == FALSE ? GetLastError() : ERROR_SUCCESS;
    operation->transferred = transferred;
    if (operation->kind == OperationKind::Write && operation->error == ERROR_SUCCESS) {
      operation->offset += transferred;
      if (operation->offset < operation->bytes.size()) {
        if (transferred == 0) operation->error = ERROR_WRITE_FAULT;
        else if (SubmitWriteRemainder(operation)) continue;
      }
    }
    CompleteNativeOperation(operation);
  }
  // The native thread owns the single TSFN release. Normal close keeps queued
  // callbacks so read/write promises settle; env teardown cannot call into JS.
  napi_release_threadsafe_function(connection->completions,
      connection->environment_closing.load() ? napi_tsfn_abort : napi_tsfn_release);
}
}  // namespace

void RequestConnectionClose(Connection* connection) {
  connection->closing.store(true);
  {
    std::lock_guard<std::mutex> lock(connection->io_mutex);
    if (connection->pipe != INVALID_HANDLE_VALUE) CancelIoEx(connection->pipe, nullptr);
  }
  if (connection->pending_native_io.load() == 0) StopNativeConnection(connection);
}
bool SubmitWriteRemainder(Operation* operation) {
  Connection* connection = operation->connection;
  std::lock_guard<std::mutex> lock(connection->io_mutex);
  if (connection->closing.load() || connection->pipe == INVALID_HANDLE_VALUE) {
    operation->error = ERROR_OPERATION_ABORTED;
    return false;
  }
  ZeroMemory(&operation->overlapped, sizeof(operation->overlapped));
  const DWORD length = static_cast<DWORD>(std::min<size_t>(
      operation->bytes.size() - operation->offset, MAXDWORD));
  const BOOL submitted = WriteFile(connection->pipe,
      operation->bytes.data() + operation->offset, length, nullptr, &operation->overlapped);
  if (submitted == FALSE) {
    const DWORD error = GetLastError();
    if (error != ERROR_IO_PENDING) {
      operation->error = error;
      return false;
    }
  }
  return true;
}
bool StartConnection(Connection* connection) {
  napi_value resource_name = nullptr;
  connection->owners.fetch_add(1);
  if (napi_create_string_utf8(connection->env, "aura-main-pipe-completions",
                              NAPI_AUTO_LENGTH, &resource_name) != napi_ok ||
      napi_create_threadsafe_function(connection->env, nullptr, nullptr, resource_name,
          2, 1, connection, FinalizeCompletions, nullptr, FinishOperationOnMain,
          &connection->completions) != napi_ok) {
    ReleaseOwner(connection);
    return false;
  }
  napi_unref_threadsafe_function(connection->env, connection->completions);
  connection->owners.fetch_add(1);
  if (napi_add_env_cleanup_hook(connection->env, CloseForEnvironment, connection) != napi_ok) {
    ReleaseOwner(connection);
    napi_release_threadsafe_function(connection->completions, napi_tsfn_abort);
    return false;
  }
  connection->cleanup_hook_registered = true;
  connection->completion_thread = std::thread([connection]() { CompletionLoop(connection); });
  return true;
}
void FinalizeConnection(napi_env env, void* data, void*) {
  auto* connection = static_cast<Connection*>(data);
  RequestConnectionClose(connection);
  if (connection->completion_thread.joinable()) connection->completion_thread.join();
  if (connection->cleanup_hook_registered) {
    napi_remove_env_cleanup_hook(env, CloseForEnvironment, connection);
    connection->cleanup_hook_registered = false;
    ReleaseOwner(connection);
  }
  if (connection->completion_port != nullptr) {
    CloseHandle(connection->completion_port);
    connection->completion_port = nullptr;
  }
  if (connection->server_process != nullptr) {
    CloseHandle(connection->server_process);
    connection->server_process = nullptr;
  }
  if (connection->wrapper != nullptr) {
    napi_delete_reference(env, connection->wrapper);
    connection->wrapper = nullptr;
    connection->wrapper_retained.store(false);
  }
  ReleaseOwner(connection);
}
bool BeginOperation(Connection* connection) {
  if (connection->pending_operations.fetch_add(1) == 0) {
    uint32_t ignored = 0;
    if (napi_reference_ref(connection->env, connection->wrapper, &ignored) != napi_ok) {
      connection->pending_operations.fetch_sub(1);
      return false;
    }
    connection->wrapper_retained.store(true);
  }
  connection->pending_native_io.fetch_add(1);
  return true;
}
void QueueSyntheticCompletion(Operation* operation) { CompleteNativeOperation(operation); }
}  // namespace aura_main_pipe

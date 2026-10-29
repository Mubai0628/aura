#pragma once
#include <node_api.h>

namespace aura_main_pipe {
// processActivity(pid, creationTime): the CPU time and completed reads and writes of the process
// with that identity, or null when it is gone or is another process now.
napi_value ProcessActivity(napi_env env, napi_callback_info info);
}  // namespace aura_main_pipe

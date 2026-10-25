# Aura 工具评测（S09 第一批）

这是产品的外部消费者。Core 和其他产品 crate 不依赖本 crate。编码任务通过生产 Electron Main/Preload 的真实 Core IPC 配置、启动、审批、读取结果及停止；不直接调用执行器。Schema 探测只经计量中转发送，不派发工具。

当前接口基线在 `fixtures/golden/`，三个适配器的最终 method、URL 和 body 由各自真实编译器生成；不保存请求头。任务目录按语言分组编号，任务清单含后续任务；可执行冒烟 R01、T01、P01 和真实规模 R06、T06、P06。后者分别处理多行日志与 UTF-8 字节预算、API 写入结果未知时的重试、依赖图循环与调度。任务只描述现象和要求，不指明实现文件。隐藏评分材料在 Agent 停止后才复制到另一份评分工作区。评分运行模型新增测试和隐藏行为测试，并检查新增测试能抓到原始错误；未补有效回归测试不能判完成。回放成功不能证明真实模型的编码效率。

真实规模任务各有多文件、已有公开测试、两处相关实现的缺陷。`reference/` 是审阅用解答，仅由离线回放配置读入，既不复制到 Agent 工作区，也不放进真实样本提示。评分要求新增或改动回归测试，然后在单独副本里还原全部相关原实现，要求公开回归变红。改了几处相关实现只记录在报告里，不作为通过条件：行为正确的修法可以比参考解改得少（例如 P06 只在图模块里拒绝循环）。Rust 只还原原有函数体，保留模型测试和新辅助项；模型改名、删除模块或留下无法解析的文件时改为整文件还原，评分继续并在日志注明。生产函数改动不算测试改动。候选最后还须通过公开和隐藏边界测试。工作区复制忽略编译与 Python 字节码缓存。R01/R06 自带 `.cargo/config.toml` 的 jobs=2 与 RUST_TEST_THREADS=2，compat Shell 不继承外层环境时也适用。

运行前使用项目当前桌面/Core 构建。所有任务工作区必须在 Aura 工作树之外，不使用 AuraTest。不会联网安装依赖。Rust 夹具没有外部依赖，使用 offline/locked。测试与运行限 CARGO_BUILD_JOBS=2、RUST_TEST_THREADS=2；评分与 Shell 中的 Cargo 必须顺序运行。

## 计量与终止

隔离评测 Provider 只配置本机计量地址，模型发现关闭。所有生成、纠错、重试、压缩等 HTTP 请求必经中转，Schema 探测也走同一中转。产品路径及三个适配器的输出没有改动。

每次请求先在 SQLite 事务里持久预留输入/输出额度，再读取现有 CredentialRef 并发送。输入采用序列化 UTF-8 字节数加协议余量的保守上界；输出必须明确且不超过批准上限。累计、运行和每请求限额同时检查；真实路径按阶段固定：S09 使用 USERPROFILE/.aura-tool-eval/S09/ledger.db（累计上限 50000000），S10 使用 USERPROFILE/.aura-tool-eval/S10/ledger.db（累计上限 150000000，所有者 2026-10-12），由运行配置的 budget_stage 选择，缺省为 S09；不能给每次真实运行另开账本。账本创建时记下累计上限，之后不能放宽。不放在 AppData：Codex 桌面应用是打包应用，它和子进程写入 AppData 的文件会被重定向到自己的包目录，换一个工具运行就会看到另一本空账本。旧的 AppData 账本未迁移时拒绝新建。按 run_id 持久记录单次限额，禁止提高已登记限额。账本故障、缺失/冲突 usage、断流或崩溃待核对一律停发，保留额度，不重放。

账本只保存请求身份、用途、预留/实际 input/output、缓存输入、供应商提供的 reasoning/cache write、工具定义字节数及数字 HTTP 状态，不记录请求头或请求/响应正文。编码请求记为 generation；同一任务内与上一条已预留请求的原始请求体完全相同则记为 retry，变化后再次出现旧请求不算相邻重试，探测始终记为 probe。中转仅在内存里保留上一条请求用于精确比较，覆盖与结束时清零；不持久保存正文或把其内容写进报告。该分类不声称识别所有压缩请求。缓存输入包括在实际 input 中，同时单列；未提供的可选明细记为未知。总量矛盾也停发。工具定义字节数是可复核的上界指标；没有供应商的分段 tokenizer 时，不伪装成精确工具 token。

中转整响应收集后核对 usage 再交 Core，因此回放/真实样本采用相同收集路径，模型流式首字延迟不代表产品原生流式延迟。供应商不兑现输出上限时只能记录超额事实并停止后续请求，不能收回已消费 token。带完整有界错误体的 4xx 按整笔预留量扣费并记录状态：探测继续下一项，编码只结束当前任务。只有探测报告保留去疑似密钥后、各最多 300 字的 error.message/code，不进账本，也不把供应商错误正文返回 Core。5xx、断流/超时、成功响应缺少或冲突 usage 仍全局停发。

运行配置固定会话模式与工作区/Shell 运行审批策略 allow_once/deny；联网与外部文件请求始终拒绝，通过真实审批 IPC 作答并计数。冒烟使用 workspace_read_write、allow_once，不持久新增会话批准；每请求输入预留上限 262144，输出与所有者档案一致（本轮为 8192，包含推理），单任务 600000、截止 1800 秒。任务额度用完只停止该任务并记 budget_terminated，重开账本也不影响别的任务；累计 50000000 用尽仍全局停止。报告记录各工具次数、参数错误类别、每请求缓存及推理、观察到的模型轮数及全部已执行 Shell 脚本。历史分页和逐轮 ID 累积避免遗漏早期命令；评分前通过用户停止 IPC 确认托管进程已结束。控制器和评分器另有有界 Job，不能确认停止时不判成功。阶段耗时按投影轮询估计，墙钟总耗时为真实计时。

compat Shell 使用用户权限，隐藏文件放在工作区外不构成系统级隔离；本框架不保证阻止恶意脚本扫描宿主评分材料。样本只处理专用副本与现有本地工具，禁止宿主配置、真实库、凭据和网络访问。

## 命令与配置

`aura-tool-eval snapshots <目录>` 只生成现有请求快照，不联网。`aura-tool-eval replay <配置.json>` 使用确定性回放，不读任何真实凭据。配置包含 task_id、run_id、root、ledger_path、desktop_root、limits、mode、approval_decision、deadline_seconds、model、request_settings 和 replay。root 为新的隔离评测根；桌面目录是本次受检 Aura 桌面构建。Responses 回放发送真实语义 SSE，含推理项；后续请求缺少该推理历史时拒绝回放。Chat 回放继续保留。

replay 项为 `{ "type":"tool", "name":"shell.powershell", "arguments":{...当前必填字段...} }` 或 `{ "type":"final", "text":"..." }`。模型信封与真实 wire 名从当前请求冻结集合组装，不手写 ToolId。仓库内没有真实账号配置；具体本机回放配置和运行命令由本轮检查索引引用。

CLI 退出成功表示运行报告已写出。任务通过还须同时满足 outcome 为 passed、IPC 确认停止，以及评分器的公共回归与隐藏行为检查；预算终止或待核对不能按 CLI 退出码当作通过。用量已结算后写回 Core 失败记为任务级 core_disconnected，已知用量不变、其他任务不因此全局停止；结果仍不重放。上游未知用量的全局停止条件不变。

`aura-tool-eval inspect-owner <source.json>` 只读取指定 models_path、database_path 与 model_profile_id 对应的模型、端点、全部非保密请求设置和已配置 CredentialRef，数据库以只读方式打开，不打开 Store、不读密钥。输出 target.json。`verify-owner <target.json>` 单独只读复核。真实运行入口是 `aura-tool-eval real <配置.json> --owner-approved-estimate <target.json>`；Schema 探测为 `aura-tool-eval probe <新隔离报告目录> --owner-approved-estimate <target.json>`。入口和中转每次发送前重新只读核对设置，变化就拒绝并要求重算。原账号配置不改；本轮复制 deepseek-flash 的 Responses、8192 输出及 256000 上下文，temperature/top_p 和 Chat 专用覆盖项也逐项保留。隔离配置只有 base URL、认证和超时不同，报告显式列出。已学习的非空兼容转换暂拒绝静默迁移。

密钥仅由 Windows Credential Manager 在预留成功后读取，并在中转内存中注入所有者现有配置中的端点，不复制到配置、环境、日志、工作区、请求快照或夹具。探测包含六个固定用例，含额外字段和枚举外取值的诱导；每个准备后的请求输入字节加协议余量必须不超过 8192，输出 8192，总硬上限 98304。任务键带隔离目录的运行编号，复测必须使用新编号。Responses 发送 input 与扁平 function；与产品一致，不发送 tool_choice（DeepSeek 思考模式拒绝强制或必选的工具选择），由提示要求调用，也不发送思考参数或 stream_options；报告保留状态、是否产生函数调用、参数校验、输出/推理 token、截断原因及有界净化错误。模型没有调用函数时如实记录，不改方式重试。一次有效输出不能证明供应商强制 strict 解码，不能据此放松 Core 校验。

两种格式只允许当前运行对应的路径。Responses usage 仅取 response.completed/response.incomplete 的 response.usage，忽略中间事件；Chat 使用现有 usage 帧。中转收齐上游响应后才把响应头和正文一起交给 Core：上游 360 秒，中转连接 480 秒。Core 的 Connect 阶段一直等到响应头，因此 Connect 与 first_event/stream_idle 都是 420 秒，total 450 秒；Connect 短于上游时间，生成稍长 Core 就会放弃并重试。任务结束时中转不再接收请求，等已收到的请求全部结算或记为未知后才出报告：Core 放弃等待的请求照样计费，用量也照样入账。报告的首事件时间因此包含整次上游生成，不能当作产品原生流式首字时间。[DeepSeek Responses 文档](https://api-docs.deepseek.com/guides/responses_api/)说明语义流与结束事件；[思考模式文档](https://api-docs.deepseek.com/guides/thinking_mode/)说明默认模式，本轮保持该默认。

首次真实调用须先通过离线检查、Responses Core IPC 回放与估算阈值。先发探测，逐条核对账本、状态码、结算和计量边界；任何条件不满足就停止。之后按 R01、T01、P01 顺序冒烟，供应商拒绝或全局停止立即结束，不自行对账、不跑正式样本。命令说明不是额外调用授权。

全局停止没有自动恢复。显式命令为 `aura-tool-eval reconcile <ledger.db> --reconciliation-file <note.json>`，文件包含 explanation 和 requests（逐项 id、usage）。说明不得为空或包含疑似密钥；必须覆盖所有 pending/unknown 请求：能确认的填写 input/output 及可选缓存明细；不能确认的把 usage 写为 null，该请求按整笔预留扣费并记为 charged_unknown。说明、旧停止原因和核对后的总量在同一事务写入账本后才恢复。原额度、历史花费、4xx 保守扣费及任务级停止都保留；累计额度已耗尽不能恢复。不编造零费用，也不记低于预留的未确认用量。

R01/T01/P01 仅用于冒烟、链路和预算校准，不能充当正式改进前样本。R06/T06/P06 每个工作区为 5–15 文件、源码加测试 300–1000 行；无外部依赖：Rust std、Node 的 node:test 与 TypeScript 类型擦除、Python unittest。回放应各自包含原样失败与参考解通过，并经真实 Core 工具完成发现、读代码、加回归、测试变红、修改两处实现、构建与测试变绿。正式“改进前”样本每个只跑一次，必须先审夹具和估算，命令说明不构成发送授权。输入预留以字节为单位，真实规模配置建议 request_input=1048576、output=8192；任务上限须留一次最大预留，不提高旧任务已登记额度。本批离线账本独立于真实共享账本，其余清单仍按后续批次实现。

目前本机生产路径为 Windows。其他系统拒绝启动 harness，避免以未实现的进程树终止能力冒充安全执行。

# Aura 本地 SVG 素材

2026-09-24：R10 会话界面已通过 `scripts/generate-lucide-icons.mjs` 从本目录生成并接入 32 个内联 Lucide 图标；其余素材保留备用。不需要重新下载或增加运行时图标依赖。

2026-10-19（L10.9）：所有者同意后，从同一固定提交补充 6 个：undo-2、corner-down-right、play、copy、chevrons-down-up、list-end。trash-2 在该提交不存在（404），没有用其他文件代替。内联 Lucide 图标现为 39 个。<!-- claude-advisor 2026-10-19 -->

2026-10-20（L10.10）：所有者同意（“Delete Icon可以下载”）后，从同一固定提交补充 trash，用于“删除”；仓库里已有的 git-branch 用于“新建分支”。内联 Lucide 图标现为 41 个。<!-- claude-advisor 2026-10-20 -->

## 内容与来源

| 目录        | 内容                       | 上游及固定版本                                                                                                                                   | 用途                                 |
| ----------- | -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------ |
| `lucide/`   | 45 个 SVG，24 × 24 viewBox | [Lucide](https://github.com/lucide-icons/lucide/tree/f06ac67e33d645c40b8ce19a0419c85c5d7dd751/icons)，`f06ac67e33d645c40b8ce19a0419c85c5d7dd751` | 工具、状态、展开、模式操作           |
| `devicon/`  | 28 个 SVG                  | [Devicon](https://github.com/devicons/devicon/tree/7330accdbc47e2dc0c19789a48533c4a3c50fe58/icons)，`7330accdbc47e2dc0c19789a48533c4a3c50fe58`   | 代码技术、文件类型和开发应用标识备用 |
| `licenses/` | 两份上游完整许可           | [Lucide 许可说明](https://lucide.dev/license)、[Devicon](https://github.com/devicons/devicon)                                                    | 分发时保留版权与许可文本             |

逐文件来源、版本、字节数、SHA-256 和 viewBox 见 [manifest.json](manifest.json)。全部 SVG 保留上游原文；VS Code 选择官方 `plain` 版本，其余 Devicon 为 `original`。初选 VS Code 原版包含滤镜，未保存该版，没有扩大静态 SVG 允许范围。

用户指定的 [iconfont 集合 21124](https://www.iconfont.cn/collections/detail?cid=21124) **尚未下载**：用户浏览器的站点访问权限拒绝了读取。本目录来自用户允许的其他知名站点，不冒称该集合，不通过 Cookie、直接请求或另一浏览器绕过拒绝。

本地 [preview.html](preview.html) 展示全部素材及 16/20/24px 的操作图标示例；它是素材预览，不是 Aura 实际窗口验收。

## 界面映射

按 Core 的稳定工具身份/展示类别映射，不能根据中文标题或模型正文猜图标。以下名称是本地 SVG 文件名；具体工具名称以冻结注册表为准。

| 语义                               | Lucide 图标                                                                     |
| ---------------------------------- | ------------------------------------------------------------------------------- |
| 列目录、目录树                     | `folder-open`、`folder-tree`                                                    |
| 读取文件、搜索文本                 | `file-text`、`file-search`                                                      |
| 修改文件、变更摘要                 | `file-pen-line`、`file-diff`                                                    |
| 命令或终端                         | `terminal`                                                                      |
| 网页、来源引用                     | `globe`、`link`                                                                 |
| 工具发现、加载、卸载               | `package-search`、`package-plus`、`package-minus`；内部管理调用仍只在详情里显示 |
| 未专门映射的工具                   | `wrench`                                                                        |
| 思考、过程展开                     | `sparkles`、`chevron-right` / `chevron-down`                                    |
| 成功、失败、未执行                 | `check`、`circle-alert`、`circle-slash`                                         |
| 取消、权限拒绝、结果未知           | `octagon-x`、`shield-alert`、`circle-question-mark`                             |
| 暂停、终止、编辑                   | `pause`、`square`、`pencil`                                                     |
| 活动、Goal、Plan                   | `activity`、`target`、`list-checks`                                             |
| 窄窗卡片入口、收拢                 | `panel-right-open`、`minimize-2`                                                |
| 排队消息：撤回、追加、继续、下一轮 | `undo-2`、`corner-down-right`、`play`、`list-end`                               |
| 复制路径、用默认应用打开、收起思考 | `copy`、`external-link`、`chevrons-down-up`                                     |
| 删除                               | `trash`                                                                         |
| 新建分支                           | `git-branch`                                                                    |

`loader-circle` 只供适合的局部加载状态备用。2026-10-20 所有者认为轮次标题前的 Orbit 方点阵“有点违和感”，交给 Claude 重新设计（L10.10）：改为细圆弧加载环，不再保留方点阵。<!-- claude-advisor 2026-10-20 -->`square` 表示终止操作，必须有可读名称，不能只凭形状区分权限。

Devicon 备用：TypeScript、JavaScript、Rust、Python、React、HTML5、CSS3、Node.js、npm、pnpm、Git、GitHub、VS Code、Docker、Chrome、Firefox、Windows、Linux、C++、C#、Java、Go、JSON、Markdown、Bash、PowerShell、Electron、Vite。品牌标识仅标明实际技术/应用，不替代 Aura 自身品牌，不表示背书；保留原比例和原色。

## 接入和排版规则

1. 通过现有构建方式引用本地资源，离线可用；不在运行时访问 CDN、下载 SVG 或加载图标字体。只打包实际引用素材，不用通配导入把整个备用库装入每个会话。
2. 操作/工具图标默认 16 × 16 CSS px，保留独立固定槽位，禁止 flex 收缩。展开箭头、工具类型、状态是不同职责，不用一个字符槽兼任三者。单行垂直居中；换行时对齐第一行，不按 SVG 基线对齐。图标与文字建议 8px 间距，行间节奏统一。
3. Lucide 使用统一线宽与 `currentColor`。外部 SVG 的 `<img>` 不会继承宿主的 `currentColor`；单色图标可用静态构建内联组件或本地 CSS mask 配合 `background-color: currentColor`。不要用运行时原文拼接/`dangerouslySetInnerHTML` 加载来源不明 SVG。Devicon 多色图标优先 `<img>`，避免不同 SVG 的内部 ID/样式碰撞。
4. 图标尺寸是 CSS 尺寸，不修改上游 path 或单独拉伸宽高。工具目标过长用可访问的截断/换行，不挤歪图标和状态；125%/150% 缩放、长中文/英文路径均需检查。
5. 有旁边文字的装饰图标 `aria-hidden`；仅图标按钮有可读名称、焦点样式和键盘操作。状态同时有文字，不只靠红绿颜色。操作按钮命中区大于图标本身。
6. 尊重 `prefers-reduced-motion`；图标本身不引入装饰性转动。保留真实状态投影，未知结果不显示成功。思考、工具调用的分组折叠须保留每个调用的目标、结果和身份。

## 维护与校验

- manifest 固定上游提交并记录原始文件 SHA-256，不使用 `main` / `latest` 作为交付版本。升级或新增时同时更新来源、散列、许可及用途；不重写现有下载记录伪造旧证据。
- Lucide 许可含 ISC 与部分 Feather 来源的 MIT 声明，Devicon 为 MIT；完整文本随素材分发。不要用一行“开源”替代版权声明。
- 静态校验已检查 XML、viewBox、散列，并排除脚本、事件属性、DTD/实体、外部资源与外部 CSS 引用。它不替代产品中的视觉和交互验证。
- 素材预览和清单不成为新的业务配置或契约事实源。使用代码放在合适的前端图标模块，工具状态与身份仍从生成契约读取。

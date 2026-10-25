import { useState, type CSSProperties } from 'react';
import type { AgentToolSettingsView } from '@aura/contracts/generated';
import { BuiltinHttpCredentials } from '../tool-credentials/BuiltinHttpCredentials';
import { ToggleSwitch } from '../ui/ToggleSwitch';

type Tool = AgentToolSettingsView['tools'][number];

/** The families tools are listed under, named by the part of the tool name before the dot. */
const FAMILY_LABELS: Record<string, string> = {
  tools: '工具目录',
  agent: '运行状况',
  mode: '模式',
  context: '上下文',
  todo: '待办',
  plan: '计划',
  goal: '目标',
  query: '提问',
  task_lineage: '任务脉络',
  normal: '轮次',
  fs: '文件',
  shell: 'Shell',
  terminal: '终端',
  process: '进程',
  git: 'Git',
  http: '网络',
  web: '网页',
  memory: '记忆',
  workspace_init: '工作区初始化',
  commands: '命令',
  skills: 'Skill',
  mcp: 'MCP',
};

const familyOf = (tool: Tool) => tool.name.split('.')[0] ?? tool.name;
const familyLabel = (family: string) => FAMILY_LABELS[family] ?? family;

/**
 * The default-tool list, grouped by family in the order the registry lists them: a row
 * of family filters, then one compact tile per tool. Every tool stays in the page — the filter
 * only narrows what is shown — and each checkbox keeps its name, so finding a tool by name and
 * the search above work as before.
 */
export function AgentToolGroups({
  tools,
  fixed,
  pending,
  onToggle,
}: {
  tools: Tool[];
  fixed: string[];
  pending: boolean;
  onToggle: (tool: Tool, checked: boolean) => void;
}) {
  const [family, setFamily] = useState('all');
  const families = new Map<string, Tool[]>();
  for (const tool of tools) {
    const key = familyOf(tool);
    families.set(key, [...(families.get(key) ?? []), tool]);
  }
  const chosen = family !== 'all' && families.has(family) ? family : 'all';
  const shown = chosen === 'all' ? [...families] : [[chosen, families.get(chosen)!] as const];
  const loaded = (tool: Tool) => tool.required || fixed.includes(tool.tool_id);
  return (
    <>
      {families.size > 1 ? (
        <div className="agent-tool-families" role="group" aria-label="按类别显示">
          {[['all', tools] as const, ...families].map(([key, items]) => (
            <button
              key={key}
              type="button"
              className="agent-tool-family"
              aria-pressed={chosen === key}
              onClick={() => setFamily(key)}
            >
              {key === 'all' ? '全部' : familyLabel(key)}
              <span>{items.length}</span>
            </button>
          ))}
        </div>
      ) : null}
      {shown.map(([key, items]) => (
        <section key={key} className="agent-tool-group" aria-label={`${familyLabel(key)}工具`}>
          <h4>
            {familyLabel(key)}
            <span>
              {items.length} 个 · 默认加载 {items.filter(loaded).length}
            </span>
          </h4>
          <ul className="agent-tool-list">
            {items.map((tool, index) => (
              <li
                key={tool.tool_id}
                data-loaded={loaded(tool) ? 'true' : undefined}
                data-wide={tool.name === 'http.request' ? 'true' : undefined}
                style={{ '--item': index } as CSSProperties}
              >
                <div>
                  <strong>{tool.name}</strong>
                  <p title={tool.description}>{tool.description}</p>
                  {tool.name === 'http.request' ? (
                    <BuiltinHttpCredentials toolId={tool.tool_id} />
                  ) : null}
                  <small>
                    {tool.source === 'builtin' ? '内置工具' : tool.source} ·{' '}
                    {tool.available ? '可用，执行时检查权限' : '不可用'}
                    {tool.required ? ' · 按任务模式加载' : ''}
                  </small>
                </div>
                <label className="agent-tool-switch">
                  <ToggleSwitch
                    checked={loaded(tool)}
                    disabled={pending || tool.required || !tool.available}
                    label={`${tool.name} 默认加载`}
                    onCheckedChange={(checked) => onToggle(tool, checked)}
                  />
                  默认加载
                </label>
              </li>
            ))}
          </ul>
        </section>
      ))}
    </>
  );
}

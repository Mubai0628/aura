import { useEffect, useState } from 'react';
import type { McpServerDetail, McpServerView } from '@aura/contracts/generated';
import { mcpError } from './mcp-labels';

/** One server's tools, the tools left out and the end of its error output. */
export function McpServerPanel({ server, refresh }: { server: McpServerView; refresh: number }) {
  const [detail, setDetail] = useState<McpServerDetail>();
  const [error, setError] = useState<string>();
  const id = server.config.id;
  useEffect(() => {
    let live = true;
    void window.aura.v1.describeMcpServer({ id }).then(
      (result) => {
        if (!live) return;
        if (result.ok) {
          setDetail(result.value);
          setError(undefined);
        } else setError(mcpError(result.error.message, '服务器详情读取失败，请重试。'));
      },
      (reason: unknown) => {
        if (live) setError(mcpError(reason, '服务器详情读取失败，请重试。'));
      },
    );
    return () => {
      live = false;
    };
  }, [id, refresh]);
  if (error) return <p role="alert">{error}</p>;
  if (!detail) return <p role="status">正在读取…</p>;
  return (
    <section className="mcp-detail" aria-label={`${server.config.display_name} 的工具`}>
      <h4>工具</h4>
      {detail.tools.length ? (
        <ul>
          {detail.tools.map((tool) => (
            <li key={tool.canonical_name}>
              <code>{tool.canonical_name}</code>
              {tool.title ? <span> {tool.title}</span> : null}
              {tool.description ? <p>{tool.description}</p> : null}
            </li>
          ))}
        </ul>
      ) : (
        <p>还没有列出工具。启动成功后，这里会显示服务器提供的工具。</p>
      )}
      {detail.skipped_tools.length ? (
        <details>
          <summary>未加入的工具（{detail.skipped_tools.length}）</summary>
          <ul>
            {detail.skipped_tools.map((reason) => (
              <li key={reason}>{reason}</li>
            ))}
          </ul>
        </details>
      ) : null}
      <details open={server.state === 'failed'}>
        <summary>
          {server.config.transport.type === 'stdio' ? '错误输出' : '日志与错误'}（最近{' '}
          {detail.stderr.length} 行）
        </summary>
        {detail.stderr.length ? <pre>{detail.stderr.join('\n')}</pre> : <p>没有错误输出。</p>}
      </details>
    </section>
  );
}

import { useEffect, useState } from 'react';
import type { ExecutionApprovalView, McpTransport } from '@aura/contracts/generated';
import type { McpServerResource } from './approval-facts';

/** The trusted configuration of the asking server, when the registry still has it unchanged. */
function useTrustedTransport(server: McpServerResource): McpTransport | undefined {
  const [transport, setTransport] = useState<McpTransport>();
  const { server_id: id, config_fingerprint: fingerprint } = server;
  useEffect(() => {
    let live = true;
    Promise.resolve()
      .then(() => window.aura.v1.listMcpServers({}))
      .then((result) => {
        if (!live || !result.ok) return;
        const trusted = result.value.servers.find(
          (view) => view.config.id === id && view.fingerprint === fingerprint.hex,
        );
        setTransport(trusted?.config.transport);
      })
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, [id, fingerprint.hex]);
  return transport;
}

/**
 * What approving an MCP tool call means. Aura does not judge what a server's tool does, so the
 * dialog says how the server runs and what each answer covers instead of implying a check.
 */
export function McpApprovalFacts({
  view,
  server,
}: {
  view: ExecutionApprovalView;
  server: McpServerResource;
}) {
  const content = view.operation.content;
  const turn = view.approval.allowed_decisions.includes('allow_for_current_turn');
  const transport = useTrustedTransport(server);
  return (
    <section aria-label="MCP 工具调用">
      <p>
        MCP 服务器「{server.server_id}」的工具 {content.action_kind} · 配置指纹{' '}
        {server.config_fingerprint.hex.slice(0, 12)}
      </p>
      <p>
        {transport?.type === 'http' ? (
          <>
            Aura 不判断这个工具会做什么：调用参数会经 Aura 的网络网关发到{' '}
            <code>{transport.url}</code>（不跟随跳转），由那里的服务器处理。
          </>
        ) : transport?.type === 'stdio' ? (
          'Aura 不判断这个工具会做什么：本机服务器以你的身份运行，能读写你的文件、访问网络。'
        ) : (
          'Aura 不判断这个工具会做什么：本机服务器以你的身份运行，能读写你的文件、访问网络；HTTP 服务器会收到这次调用的参数。'
        )}
        工具的结果会作为外部内容交给模型，不当作指令。
      </p>
      {turn ? (
        <p>
          「本轮允许」覆盖这个服务器在本轮内的后续调用；「拒绝」后本轮内这个服务器的调用都会直接被拒绝，不再询问。下一轮会重新询问。
        </p>
      ) : null}
      <details>
        <summary>调用参数</summary>
        <pre>{JSON.stringify(content.action, null, 2)}</pre>
      </details>
    </section>
  );
}

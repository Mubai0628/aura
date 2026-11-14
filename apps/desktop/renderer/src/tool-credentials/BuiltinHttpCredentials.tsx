import { answerMissing, connectionCause, refusedAction } from '../agent-session/failure-text';
import { useMemo, useState } from 'react';
import type { ToolId } from '@aura/contracts/generated';
import { useScopedToolProjection, useScopedToolProjectionFailure } from '../tool-projection-store';
import { projectionNotRead } from '../tool-settings/failure-words';
import {
  ToolCredentialBindingControl,
  type ToolMutationRunner,
} from './ToolCredentialBindingControl';

function Details({ toolId }: { toolId: ToolId }) {
  const route = useMemo(
    () =>
      ({
        projection_kind: 'tool_definition',
        projection_scope: { type: 'tool_definition', tool_id: toolId },
      }) as const,
    [toolId],
  );
  const view = useScopedToolProjection(route)?.payload;
  const failure = useScopedToolProjectionFailure(route);
  const [busy, setBusy] = useState<string>();
  const [message, setMessage] = useState<string>();
  const runMutation: ToolMutationRunner = async (key, operation, title) => {
    if (busy) return;
    setBusy(key);
    try {
      const result = await operation();
      setMessage(result.ok ? title : bindingFailure(key, result.error));
      await window.aura.v1.refreshToolProjection(route);
    } catch {
      setMessage('操作结果未确认，请刷新凭据状态。');
    } finally {
      setBusy(undefined);
    }
  };
  const instances = view?.versions
    .at(-1)
    ?.instances.filter((instance) => instance.credential_slot_binding != null);
  return (
    <section aria-label="HTTP 工具凭据">
      <p>
        http.request 的 api_key
        使用下方明确绑定的工具凭据。允许网站由你配置；联网审批不会扩大密钥使用范围。
        内置工具更新会沿用已有配置；只有凭据槽或放置方式变化时才需要重新绑定并核对允许网站。
      </p>
      {message ? <p role="status">{message}</p> : null}
      {!instances ? (
        <p role="status">
          {failure === undefined
            ? '正在读取 HTTP 工具凭据绑定…'
            : projectionNotRead('HTTP 工具凭据绑定', failure)}
        </p>
      ) : (
        instances.map((instance) =>
          instance.credential_slot_binding ? (
            <div key={instance.tool_instance_id}>
              <ToolCredentialBindingControl
                boundCredential={instance.credential_slot_binding.bound_credential}
                busyAction={busy}
                credential={instance.credential_slot_binding}
                instance={instance}
                mutable
                runMutation={runMutation}
              />
              {instance.credential_slot_binding.bound_credential ? (
                <button
                  type="button"
                  disabled={busy !== undefined}
                  onClick={() =>
                    void runMutation(
                      `clear:${instance.tool_instance_id}`,
                      () =>
                        window.aura.v1.clearToolInstanceCredential({
                          tool_instance_id: instance.tool_instance_id,
                          expected_entity_revision: instance.entity_revision,
                          expected_binding_revision:
                            instance.credential_slot_binding!.bound_credential!.binding_revision,
                        }),
                      'HTTP 凭据绑定已清除',
                    )
                  }
                >
                  清除 HTTP 凭据绑定
                </button>
              ) : null}
            </div>
          ) : null,
        )
      )}
    </section>
  );
}
/** A bind, or a clear (`key` starting `clear:`), that did not happen or was not answered. */
function bindingFailure(key: string, error: unknown): string {
  const clear = key.startsWith('clear:');
  if (answerMissing(error))
    return `没有等到 Aura Core 的答复（${connectionCause(error)}），${
      clear ? '绑定可能已经清除' : '凭据可能已经绑定'
    }；请收起后重新打开这里核对。`;
  return clear
    ? refusedAction(error, '绑定没有清除', 'HTTP 工具仍使用原来的凭据；请重试。')
    : refusedAction(error, '凭据没有绑定', 'HTTP 工具的绑定保持原样；请重试。');
}

export function BuiltinHttpCredentials({ toolId }: { toolId: ToolId }) {
  const [open, setOpen] = useState(false);
  return (
    <div>
      <button type="button" aria-expanded={open} onClick={() => setOpen((value) => !value)}>
        管理 HTTP 凭据与允许网站
      </button>
      {open ? <Details toolId={toolId} /> : null}
    </div>
  );
}

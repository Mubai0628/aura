import type { PermissionMode } from '@aura/contracts/generated';
import { useEffect, useRef, useState } from 'react';
import { ConfirmationDialog } from './ConfirmationDialog';
import { ResizableComposer } from './ResizableComposer';
import { readSessionModelSelector } from './model-projection-store';
import {
  getComposerDraft,
  updateComposerDraft,
  type ComposerDraft,
} from './agent-session/composer-drafts';
import { rememberFirstTitle } from './agent-session/session-titles';
import { useDraftModelChoices } from './agent-session/useDraftModelChoices';
import { userMessage } from './agent-session/user-message';
import { OceanBackdrop } from './ocean/OceanBackdrop';
import { ConversationAttachments } from './composer-attachments/ConversationAttachments';
import { FULL_ACCESS_DELETES } from './security/full-access-notice';
import { SendingMessage } from './agent-session/SendingMessage';
import { useSending } from './agent-session/sending';

export function NewSessionView({
  draft,
  onOpenSettings,
  onAccepted,
  onInspect,
}: {
  draft: ComposerDraft;
  onOpenSettings: (section: 'models' | 'security' | 'tools' | 'memory') => void;
  onAccepted: (localId: string, sessionId: string) => void;
  onInspect: (sessionId: string) => void;
}) {
  const catalog = useDraftModelChoices();
  const sending = useSending(draft.id);
  const [defaultMode, setDefaultMode] = useState<PermissionMode>();
  const [confirmFull, setConfirmFull] = useState(false);
  const binding = useRef<{ sessionId: string; revision: string; modelId: string } | undefined>(
    undefined,
  );
  useEffect(() => {
    let active = true;
    void window.aura.v1
      .getPermissionDefaults()
      .then((result) => {
        if (active && result.ok) setDefaultMode(result.value.mode);
      })
      .catch(() => undefined);
    return () => {
      active = false;
    };
  }, []);
  const prepare = async (modelId: string) => {
    const current = getComposerDraft(draft.id) ?? draft;
    let id = current.serverSessionId;
    if (!id) {
      const result = await window.aura.v1.createSession({
        workspace_id: draft.workspaceId!,
        title: '',
      });
      if (!result.ok)
        throw new Error(userMessage(result.error.message, '会话创建未确认，草稿已保留。'));
      id = result.value.session_id;
      updateComposerDraft(draft.id, { serverSessionId: id });
    }
    const selector =
      binding.current?.sessionId === id
        ? binding.current
        : await readSessionModelSelector(id).then((value) => ({
            sessionId: id,
            revision: value.payload.binding_revision,
            modelId: value.payload.selected_model_profile_id ?? '',
          }));
    if (selector.modelId !== modelId) {
      const result = await window.aura.v1.setSessionModelBinding({
        session_id: id,
        model_profile_id: modelId,
        expected_revision: selector.revision,
      });
      if (!result.ok)
        throw new Error(
          userMessage(result.error.message, '模型配置尚未就绪，请在设置中检查凭据与模型。'),
        );
      binding.current = { sessionId: id, revision: result.value.revision, modelId };
    }
    if (current.permissionMode) {
      const permission = await window.aura.v1.getSessionPermissionState({ session_id: id });
      if (!permission.ok) throw new Error('未能读取会话权限，消息尚未发送。');
      if (permission.value.mode !== current.permissionMode) {
        const result = await window.aura.v1.setSessionPermissionMode({
          session_id: id,
          mode: current.permissionMode,
          expected_revision: permission.value.revision,
        });
        if (!result.ok) throw new Error('权限设置未完成，消息尚未发送。');
      }
    }
    return id;
  };
  return (
    <ConversationAttachments
      key={draft.id}
      className="conversation-workspace new-session-workspace"
    >
      <header className="conversation-header">
        <h2>新会话</h2>
      </header>
      <div className="new-session-body">
        <OceanBackdrop />
        {sending ? (
          <div className="new-session-sending">
            <SendingMessage sending={sending} />
          </div>
        ) : (
          <div className="new-session-welcome">
            <span>AURA</span>
            {/* No subtitle under the title. */}
            <h1>从一个想法开始</h1>
          </div>
        )}
        <div className="new-session-composer">
          {draft.serverSessionId && draft.needsReview && (
            <button
              type="button"
              className="draft-inspect"
              onClick={() => onInspect(draft.serverSessionId!)}
            >
              查看消息历史与队列
            </button>
          )}
          <ResizableComposer
            sessionId={draft.id}
            queuePending={false}
            onChanged={() => undefined}
            onStop={async () => undefined}
            demoMode={false}
            modelSelector={undefined}
            draftModelPicker={{ ...catalog, selectedId: draft.modelId ?? catalog.defaultId }}
            preparation={{
              prepare,
              accepted: (id, text) => {
                rememberFirstTitle(id, text);
                onAccepted(draft.id, id);
              },
            }}
            onSelectModel={async (modelId) => {
              updateComposerDraft(draft.id, { modelId });
            }}
            onOpenModelSettings={() => onOpenSettings('models')}
            onOpenPromptInspector={() => undefined}
            onOpenSecuritySettings={() => onOpenSettings('security')}
            onSetPermissionMode={async (mode) => {
              if (mode === 'full_access') setConfirmFull(true);
              else updateComposerDraft(draft.id, { permissionMode: mode });
            }}
            permissionMode={draft.permissionMode ?? defaultMode}
            promptInspector={undefined}
            workspaceId={draft.workspaceId}
            onOpenSettings={onOpenSettings}
          />
        </div>
      </div>
      {confirmFull && (
        <ConfirmationDialog
          busy={false}
          title="启用本会话的完全访问"
          titleId="draft-full-access-title"
          confirmLabel="启用完全访问"
          description={`模型可联网，并在已授权范围内修改或删除文件。请仅对你信任的任务启用。${FULL_ACCESS_DELETES}`}
          onCancel={() => setConfirmFull(false)}
          onConfirm={() => {
            updateComposerDraft(draft.id, { permissionMode: 'full_access' });
            setConfirmFull(false);
          }}
        />
      )}
    </ConversationAttachments>
  );
}

import { useId } from 'react';
import { AppIcon } from '../app-icons';
import { ModalDialog } from '../ModalDialog';
import { useAgentProfile } from './useAgentProfile';
import '../model-dialogs.css';
import './agent-profile.css';

export function AgentProfileDialog({
  sessionId,
  onClose,
}: {
  sessionId: string;
  onClose: () => void;
}) {
  const titleId = useId();
  return (
    <ModalDialog className="agent-profile-dialog" titleId={titleId} onClose={onClose}>
      <header className="modal-header">
        <h2 id={titleId}>编辑会话简介</h2>
        <button aria-label="关闭" className="icon-button" type="button" onClick={onClose}>
          <AppIcon name="close" />
        </button>
      </header>
      <AgentProfileEditor sessionId={sessionId} visible />
    </ModalDialog>
  );
}

export function AgentProfileEditor({
  sessionId,
  visible,
}: {
  sessionId: string;
  visible: boolean;
}) {
  const id = useId();
  const state = useAgentProfile(sessionId, visible);
  const { profile, draft, problem, busy, dirty } = state;
  const candidate = profile?.generation?.preview;
  if (!state.available) return <p role="status">会话简介暂不可用</p>;
  return (
    <section aria-label="会话 Agent 简介" className="agent-profile">
      <label htmlFor={id}>会话职责简介</label>
      <textarea
        id={id}
        rows={2}
        maxLength={1200}
        value={draft}
        disabled={!profile || busy}
        onChange={(event) => state.edit(event.target.value)}
      />
      <div className="agent-profile-actions">
        <button
          className="button is-small"
          type="button"
          disabled={!profile || busy || !dirty}
          onClick={() => void state.save()}
        >
          {busy ? '正在处理简介…' : '保存简介'}
        </button>
        <button className="button is-small" type="button" disabled={busy} onClick={state.refresh}>
          {dirty ? '放弃编辑并重新读取' : '重新读取'}
        </button>
        <button
          className="button is-small"
          type="button"
          disabled={
            !profile ||
            busy ||
            dirty ||
            !state.canPreview ||
            profile.generation?.state === 'running'
          }
          onClick={() => void state.preview()}
        >
          重新生成建议
        </button>
      </div>
      {candidate ? (
        <div aria-label="生成的简介建议">
          <p className="agent-profile-suggestion">{candidate.description}</p>
          <button
            className="button is-small"
            type="button"
            disabled={busy || candidate.base_revision !== profile?.revision}
            onClick={() => state.adopt(candidate)}
          >
            采用为编辑内容
          </button>
          {candidate.base_revision !== profile?.revision ? (
            <p role="status">简介已经更新，这份建议基于旧版，请重新生成。</p>
          ) : null}
        </div>
      ) : null}
      {!profile && !problem ? <p role="status">正在读取会话简介…</p> : null}
      {profile && (profile.input_tokens || profile.output_tokens || !profile.usage_complete) ? (
        <p className="agent-profile-usage">
          简介生成用量：输入 {profile.input_tokens.toLocaleString()}，输出{' '}
          {profile.output_tokens.toLocaleString()}
          {profile.usage_complete ? '' : ' · 待核对'}
        </p>
      ) : null}
      {profile?.generation?.state === 'running' ? <p role="status">正在生成简介…</p> : null}
      {profile?.generation?.message ? <p role="status">{profile.generation.message}</p> : null}
      {problem ? <p role="alert">{problem}</p> : null}
    </section>
  );
}

import { userMessage } from '../agent-session/user-message';
import { useState } from 'react';
import { ToolCredentialManager } from './ToolCredentialManager';
import type {
  ToolBoundCredentialSummaryV1,
  ToolCredentialCandidatesProjectionV1,
  ToolDefinitionInstanceView,
  ToolMutationResultV1,
} from '@aura/contracts/generated';
import type { ResultEnvelope } from '../../../shared/aura-api';
export type ToolMutationRunner = (
  actionKey: string,
  operation: () => Promise<ResultEnvelope<ToolMutationResultV1 | null>>,
  successTitle: string,
  onCommitted?: (result: ToolMutationResultV1) => void,
) => Promise<void>;

export function ToolCredentialBindingControl({
  boundCredential,
  busyAction,
  credential,
  instance,
  mutable,
  runMutation,
}: {
  boundCredential: ToolBoundCredentialSummaryV1 | null | undefined;
  busyAction: string | undefined;
  credential: NonNullable<ToolDefinitionInstanceView['credential_slot_binding']>;
  instance: ToolDefinitionInstanceView;
  mutable: boolean;
  runMutation: ToolMutationRunner;
}) {
  const [candidates, setCandidates] = useState<ToolCredentialCandidatesProjectionV1>();
  const [candidateError, setCandidateError] = useState<string>();
  const [loading, setLoading] = useState(false);
  const [selectedCredentialId, setSelectedCredentialId] = useState('');
  const selected = candidates?.candidates.find(
    (candidate) => candidate.credential_id === selectedCredentialId,
  );
  const stale =
    candidates !== undefined && candidates.instance_revision !== instance.entity_revision;

  const loadCandidates = async (): Promise<void> => {
    if (loading) return;
    setLoading(true);
    setCandidateError(undefined);
    try {
      const result = await window.aura.v1.getToolCredentialCandidates({
        credential_slot_key: credential.slot_key,
        expected_instance_revision: instance.entity_revision,
        placement: credential.placement,
        session_id: null,
        tool_instance_id: instance.tool_instance_id,
        workspace_id: null,
      });
      if (!result.ok) {
        setCandidates(undefined);
        setSelectedCredentialId('');
        setCandidateError(userMessage(result.error, 'Credential 候选查询失败。'));
        return;
      }
      setCandidates(result.value);
      setSelectedCredentialId((current) =>
        result.value.candidates.some((candidate) => candidate.credential_id === current)
          ? current
          : '',
      );
    } catch (error) {
      setCandidates(undefined);
      setSelectedCredentialId('');
      setCandidateError(userMessage(error, 'Credential 候选查询失败。'));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="tool-credential-binding-control">
      <ToolCredentialManager
        key={`${instance.tool_instance_id}:${credential.slot_key}`}
        disabled={busyAction !== undefined || !mutable}
        target={{
          tool_instance_id: instance.tool_instance_id,
          expected_instance_revision: instance.entity_revision,
          credential_slot_key: credential.slot_key,
          placement: credential.placement,
          workspace_id: null,
          session_id: null,
        }}
        onChanged={loadCandidates}
      />
      <button
        disabled={busyAction !== undefined || loading || !mutable}
        onClick={() => void loadCandidates()}
        type="button"
      >
        {loading ? '正在读取候选…' : '读取可用 Credential'}
      </button>
      {candidateError === undefined ? null : (
        <p className="tool-detail-error" role="alert">
          {candidateError}
        </p>
      )}
      {credential.rebind_required_reason ? (
        <p className="tool-detail-error" role="alert">
          {credential.rebind_required_reason === 'credential_slot_changed'
            ? '凭据槽或放置方式已变化。请在工具设置中重新绑定凭据，并核对允许网站。'
            : '凭据绑定需要核对。请在工具设置中重新绑定凭据，并核对允许网站。'}
        </p>
      ) : null}
      {candidates === undefined ? (
        <p className="tool-contract-wait">
          新绑定只会从 Core 按用户、作用域、Slot 与策略筛选的候选集中选择；不接受手工 Credential
          ID。
        </p>
      ) : candidates.candidates.length === 0 ? (
        <p className="tool-inline-empty">当前作用域没有兼容的 Tool Credential。</p>
      ) : (
        <form
          onSubmit={(event) => {
            event.preventDefault();
            if (selected === undefined || stale) return;
            void runMutation(
              `instance-credential-bind:${instance.tool_instance_id}:${selected.credential_id}`,
              () =>
                window.aura.v1.bindToolInstanceCredential({
                  credential_id: selected.credential_id,
                  credential_revision: selected.credential_revision,
                  credential_slot_key: candidates.credential_slot_key,
                  expected_binding_revision: boundCredential?.binding_revision ?? null,
                  expected_candidate_set_revision: candidates.candidate_set_revision,
                  expected_entity_revision: candidates.instance_revision,
                  placement: candidates.placement,
                  ...(candidates.session_id === undefined
                    ? {}
                    : { session_id: candidates.session_id }),
                  tool_instance_id: candidates.tool_instance_id,
                  ...(candidates.workspace_id === undefined
                    ? {}
                    : { workspace_id: candidates.workspace_id }),
                }),
              'Credential 绑定已提交',
            );
          }}
        >
          <label>
            <span>Credential</span>
            <select
              disabled={busyAction !== undefined || !mutable || stale}
              onChange={(event) => setSelectedCredentialId(event.target.value)}
              value={selectedCredentialId}
            >
              <option value="">请选择 Core 认证候选</option>
              {candidates.candidates.map((candidate) => (
                <option key={candidate.credential_id} value={candidate.credential_id}>
                  {candidate.display_name} · {candidate.credential_kind} ·{' '}
                  {candidate.scope_summary.scope}
                </option>
              ))}
            </select>
          </label>
          {stale ? (
            <p className="tool-detail-error" role="alert">
              实例修订已变化，请重新读取候选。
            </p>
          ) : null}
          <button
            disabled={busyAction !== undefined || !mutable || stale || selected === undefined}
            type="submit"
          >
            绑定所选 Credential
          </button>
        </form>
      )}
    </div>
  );
}

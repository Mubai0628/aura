import { useState } from 'react';
import { ToolCredentialManager } from './ToolCredentialManager';
import type {
  ToolBoundCredentialSummaryV1,
  ToolCredentialCandidatesProjectionV1,
  ToolDefinitionInstanceView,
  ToolMutationResultV1,
} from '@aura/contracts/generated';
import type { ResultEnvelope } from '../../../shared/aura-api';
import { refusedAction } from '../agent-session/failure-text';
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
        setCandidateError(candidatesNotRead(result.error));
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
      setCandidateError(candidatesNotRead(error));
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
        {loading ? '正在读取…' : '读取可用的凭据'}
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
      {candidates === undefined ? null : candidates.candidates.length === 0 ? (
        <p className="tool-inline-empty">没有可用于这个工具的凭据。</p>
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
              '凭据已绑定',
            );
          }}
        >
          <label>
            <span>凭据</span>
            <select
              disabled={busyAction !== undefined || !mutable || stale}
              onChange={(event) => setSelectedCredentialId(event.target.value)}
              value={selectedCredentialId}
            >
              <option value="">请选择凭据</option>
              {candidates.candidates.map((candidate) => (
                <option key={candidate.credential_id} value={candidate.credential_id}>
                  {`${candidate.display_name} · ${KIND[candidate.credential_kind] ?? candidate.credential_kind} · ${SCOPE[candidate.scope_summary.scope]}`}
                </option>
              ))}
            </select>
          </label>
          {stale ? (
            <p className="tool-detail-error" role="alert">
              这个工具刚有变化，请重新点“读取可用的凭据”。
            </p>
          ) : null}
          <button
            disabled={busyAction !== undefined || !mutable || stale || selected === undefined}
            type="submit"
          >
            绑定所选凭据
          </button>
        </form>
      )}
    </div>
  );
}

const KIND: Readonly<Record<string, string>> = { api_key: 'API Key' };
const SCOPE: Readonly<Record<string, string>> = {
  user: '仅本用户',
  workspace: '仅本工作区',
  session: '仅本会话',
};

/** Credentials to bind that were not read: why, and that binding waits. */
const candidatesNotRead = (error: unknown): string =>
  refusedAction(
    error instanceof Error ? undefined : error,
    '可用的凭据没有读到',
    '暂时不能绑定；请稍后重试。',
  );

// Fixed coding-task policy: no network or external file grant is created by automation.
export async function answerApproval(page, session_id, approval_id, configuredDecision) {
  return page.evaluate(
    async ({ session_id, approval_id, configuredDecision }) => {
      const api = globalThis.window.aura.v1;
      const r = await api.getAgentApproval({ session_id, approval_id });
      if (!r.ok) throw new Error(r.error.code);
      const view = r.value;
      const requested = view.requested_capabilities ?? view.operation.content.capabilities;
      const network = requested.includes('ordinary_network');
      const fileAccess = requested.some((cap) => cap.includes('file_'));
      const outside =
        fileAccess &&
        view.operation.content.resources.some((r) =>
          /^(external|system|sensitive)/.test(r.scope_kind),
        );
      const decision = network || outside ? 'deny' : configuredDecision;
      const answer = await api.resolveAgentApproval({
        session_id,
        approval_id,
        expected_revision: view.approval.revision,
        decision,
      });
      if (!answer.ok) throw new Error(answer.error.code);
      return { approval_id, decision, requested_capabilities: requested };
    },
    { session_id, approval_id, configuredDecision },
  );
}

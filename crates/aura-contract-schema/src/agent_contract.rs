//! Aggregate the owning Rust types; no duplicate public DTO definitions.
pub(super) fn register(generator: &mut schemars::SchemaGenerator) {
    use aura_agent_domain::*;
    use aura_context_domain::{AgentContextReadEventsInput, AgentContextReadToolResultInput};
    use aura_projection_contract::AgentSessionProjection;
    use aura_windows_user_contract::ShellExecuteInput;
    use aura_workspace_domain::*;
    macro_rules! register { ($($ty:ty),* $(,)?) => { $(generator.subschema_for::<$ty>();)* } }
    register!(
        aura_windows_user_contract::UserTerminalOpenRequestV1,
        aura_windows_user_contract::TerminalSessionRequestV1,
        aura_windows_user_contract::TerminalRequestV1,
        aura_windows_user_contract::UserTerminalWriteRequestV1,
        aura_windows_user_contract::TerminalResizeRequestV1,
        aura_windows_user_contract::TerminalSubscribeRequestV1,
        aura_windows_user_contract::TerminalViewV1,
        aura_windows_user_contract::TerminalListV1,
        aura_windows_user_contract::TerminalOutputBatchV1,
        aura_windows_user_contract::TerminalOpeningStartV1,
        aura_windows_user_contract::TerminalOpeningRefV1,
        aura_windows_user_contract::TerminalOpeningProgressV1,
        aura_windows_user_contract::TerminalOpeningChangeV1,
        aura_runtime_contract::CancelDisposition,
        aura_windows_user_contract::TerminalControlRequestV1,
        aura_windows_user_contract::TerminalStateChangedV1,
        aura_windows_user_contract::AgentTerminalOpenInput,
        aura_windows_user_contract::AgentTerminalWriteInput,
        aura_windows_user_contract::AgentTerminalReadInput,
        aura_windows_user_contract::AgentTerminalCloseInput,
        aura_windows_user_contract::AgentTerminalResultV1,
        aura_windows_user_contract::AgentTerminalSensitiveInput,
        aura_windows_user_contract::TerminalSensitiveResultV1,
        aura_windows_user_contract::TerminalSensitiveRequestV1,
        aura_windows_user_contract::TerminalSensitiveReplyV1,
        aura_windows_user_contract::TerminalSensitivePendingV1,
        aura_windows_user_contract::TerminalSensitiveChangedV1,
        aura_windows_user_contract::ManagedProcessRequestV1,
        aura_windows_user_contract::ManagedProcessStopRequestV1,
        aura_windows_user_contract::ManagedProcessListV1,
        aura_windows_user_contract::ManagedProcessStopResultV1,
        aura_tool_contract::AgentNetworkProxyDraftV1,
        aura_tool_contract::AgentNetworkProxyTestRequest,
        aura_tool_contract::AgentNetworkProxyTestResult,
        aura_tool_contract::AgentToolSettingsRequest,
        aura_tool_contract::AgentToolSettingsUpdate,
        aura_tool_contract::AgentToolSettingsView,
        aura_tool_contract::SessionDynamicTools,
        SessionCreateRequest,
        SessionRenameRequest,
        SessionLifecycleRequest,
        SessionDeleteResult,
        aura_config::UiSettingsSnapshot,
        aura_config::UiSettingsUpdate,
        aura_config::BrowserSettingsSnapshot,
        aura_config::BrowserSettingsUpdate,
        WorkspaceSelectDirectoryRequest,
        WorkspaceDirectorySelection,
        AgentStartRequest,
        AgentInitStartRequest,
        AgentCompactionStartRequest,
        AgentTaskPurpose,
        CommandCatalogRequest,
        CommandCatalog,
        CommandSetEnabledRequest,
        CommandDeleteRequest,
        CommandDirectoryRequest,
        CommandDirectoryOpened,
        WorkspaceFileOpener,
        WorkspaceFileOpenRequest,
        ExternalLinkOpenRequest,
        OpenedOutside,
        ClipboardWriteRequest,
        ClipboardWritten,
        WorkspaceIndexRebuildRequest,
        WorkspaceIndexRebuildStarted,
        AgentSessionBranchRequest,
        AgentBranchOrigin,
        AgentSessionBranchResult,
        AgentSessionsActivityRequest,
        AgentSessionsActivity,
        AgentSessionActivity,
        AgentSessionActivityState,
        TurnReasoningEffort,
        aura_context_domain::AgentCompactionRequest,
        aura_context_domain::AgentCompactionStatus,
        aura_context_domain::AgentContextRecheckRequest,
        aura_context_domain::AgentContextRemediationView,
        aura_model_domain::AgentContextWindowResolveRequest,
        AgentSessionRequest,
        AgentSubmitRequest,
        AgentInboxMutation,
        AgentInboxProjection,
        AgentHistoryRequest,
        AgentMessageTextRequest,
        AgentMessageTextPage,
        AgentSessionProjection,
        AgentResultRequest,
        AgentResultView,
        AgentInspectorRequest,
        AgentRelatedTurnRequest,
        AgentQueryFileRequest,
        AgentQueryFileSelection,
        aura_context_domain::AgentPromptInspection,
        AgentQueryAnswerRequest,
        AgentQueryTouchRequest,
        AgentQueryTouchView,
        AgentBranchCopyingView,
        AgentModeControlRequest,
        AgentModeEpisodeView,
        aura_execution_contract::ExecutionApprovalRequest,
        aura_execution_contract::ExecutionApprovalResolutionRequest,
        aura_execution_contract::ExecutionApprovalView,
        AgentRunRequest,
        AgentRunProjection,
        TurnTask,
        AgentRun,
        ModelStep,
        RecoveryEpisode,
        OriginalUserMessage,
        TaskLineage,
        TaskLineageReconcileInput,
        TaskLineageReconcileToolInput,
        TaskLineageListOpenResult,
        TaskLineageReadSourcesInput,
        TaskLineageReadSourcesResult,
        TaskLineageReviewCompletionInput,
        CompletionReview,
        SourceReadReceipt,
        NormalCompleteInput,
        NormalCompletionRecord,
        TodoState,
        TodoReplaceInput,
        TodoUpdateInput,
        PlanCompleteInput,
        PlanRecord,
        GoalState,
        GoalCheckInput,
        GoalCompleteInput,
        QueryUserQuestionInput,
        UserQuery,
        QueryAnswer,
        EmptyToolInput,
        AgentFileReadInput,
        AgentFileListInput,
        AgentFileListResult,
        AgentFileSearchInput,
        AgentFileWriteInput,
        AgentFileDeleteInput,
        AgentFileDeleteResult,
        AgentFilePatchInput,
        AgentContextReadEventsInput,
        AgentContextReadToolResultInput,
        aura_context_domain::AgentContextReadCompactionInput,
        aura_windows_user_contract::ShellQueryRequest,
        aura_windows_user_contract::ShellStopRequest,
        aura_windows_user_contract::ShellExecutionView,
        aura_tool_projection::AgentToolActivityRequest,
        aura_tool_projection::AgentToolActivityPage,
        // The run-information query contract. The Rust DTOs are the single
        // source; the JSON schema and the TypeScript validators are generated from them.
        aura_activity_domain::ActivityQueryRequestV1,
        aura_activity_domain::ActivityQueryResponseV1,
        ShellExecuteInput
    );
}

#[cfg(test)]
mod tests {
    use aura_workspace_domain::{WorkspaceDirectorySelection, WorkspaceSelectDirectoryRequest};
    use serde_json::{from_value, json, to_value};

    #[test]
    fn workspace_directory_selection_has_no_renderer_path_or_cancelled_selection()
    -> Result<(), serde_json::Error> {
        assert!(from_value::<WorkspaceSelectDirectoryRequest>(json!({})).is_ok());
        assert!(
            from_value::<WorkspaceSelectDirectoryRequest>(json!({"selected_root": "C:/other"}))
                .is_err()
        );
        let selected = WorkspaceDirectorySelection::Selected {
            selected_root: "C:/synthetic/工作区".into(),
            suggested_display_name: "工作区".into(),
        };
        assert_eq!(from_value::<WorkspaceDirectorySelection>(to_value(&selected)?)?, selected);
        assert_eq!(
            to_value(WorkspaceDirectorySelection::Cancelled {})?,
            json!({"status": "cancelled"})
        );
        assert!(
            from_value::<WorkspaceDirectorySelection>(
                json!({"status": "cancelled", "selected_root": "C:/other"})
            )
            .is_err()
        );
        Ok(())
    }
}

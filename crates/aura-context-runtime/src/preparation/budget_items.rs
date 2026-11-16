//! The parts a request's budget counts, in the order they are sent, each with the section and the
//! use it is shown under.
use super::*;
use crate::current_user::current_user_sections;
use crate::estimator::estimate_native_reasoning;
use crate::{BudgetTextKind, CanonicalTextBudgetItem, NativeAttachmentBudgetItem};
use aura_model_domain::LogicalInstructionKind;
use aura_model_transport::{
    ProviderTools, historical_tool_call_wire_bindings, provider_history_replay,
    provider_reasoning_content, provider_tool_call,
};

#[derive(Clone, Copy, Debug)]
pub(super) struct BudgetAssignment {
    pub(super) usage: ContextUsageKind,
    pub(super) section: ContextSectionKind,
    pub(super) compressibility: ContextCompressibility,
    pub(super) tool_index: Option<usize>,
    pub(super) attachment_index: Option<usize>,
}

#[derive(Clone, Debug)]
enum PendingBudgetPayload {
    Text(String, BudgetTextKind),
    NativeAttachment(ModelAttachmentSnapshot),
}

#[derive(Clone, Debug)]
pub(super) struct PendingBudgetItem {
    pub(super) ordinal: u32,
    payload: PendingBudgetPayload,
    pub(super) assignment: BudgetAssignment,
}

pub(super) struct RequestBudgetParts<'a> {
    pub(super) recent_history: &'a [LogicalConversationItem],
    pub(super) new_users: &'a [LogicalConversationItem],
    pub(super) split_user_sections: bool,
    pub(super) provider: Option<&'a ModelExecutionSnapshot>,
    pub(super) closing_question: Option<&'a str>,
    pub(super) replayed_task_states: Vec<String>,
}

pub(super) fn build_pending_budget_items(
    assembly: &ContextSectionAssembly,
    schema_set: &ToolSchemaSetSnapshotV1,
    tools: &[ContextToolInput],
    attachments: &[ModelAttachmentSnapshot],
    request: RequestBudgetParts<'_>,
) -> Result<Vec<PendingBudgetItem>, ContextPreparationError> {
    let mut items = Vec::new();
    for (index, block) in assembly.instruction_envelope.ordered_blocks.iter().enumerate() {
        let section = instruction_section(block.kind);
        let mut content = block.content.clone();
        if block.kind != LogicalInstructionKind::TaskState
            && assembly
                .instruction_envelope
                .ordered_blocks
                .get(index + 1)
                .is_some_and(|next| next.kind != LogicalInstructionKind::TaskState)
        {
            content.push('\n');
        }
        push_text_budget_item(
            &mut items,
            content,
            BudgetTextKind::Text,
            instruction_usage(block.kind),
            section,
            budget_compressibility(assembly, section)?,
            None,
        )?;
    }
    for state in request.replayed_task_states {
        push_text_budget_item(
            &mut items,
            state,
            BudgetTextKind::Text,
            ContextUsageKind::Conversation,
            ContextSectionKind::CurrentTaskState,
            ContextCompressibility::NeverCompress,
            None,
        )?;
    }
    let wire_tools = request
        .provider
        .map(|snapshot| ProviderTools::compile(schema_set, snapshot.api_format))
        .transpose()?;
    let mut definitions = wire_tools.as_ref().into_iter().flat_map(ProviderTools::definitions);
    for (index, (tool, schema)) in tools.iter().zip(&schema_set.tools).enumerate() {
        let content = match definitions.next() {
            Some(definition) => wire_string(definition)?,
            None => canonical_string(schema)?,
        };
        push_text_budget_item(
            &mut items,
            content,
            BudgetTextKind::Text,
            ContextUsageKind::ToolsAndResults,
            ContextSectionKind::LoadedToolSchemas,
            tool.compressibility,
            Some(index),
        )?;
    }
    let bindings = request.provider.map(historical_tool_call_wire_bindings);
    for item in request.recent_history {
        match item {
            LogicalConversationItem::User { content, .. } => push_text_budget_item(
                &mut items,
                content.clone(),
                BudgetTextKind::Text,
                ContextUsageKind::Conversation,
                ContextSectionKind::RecentHistory,
                ContextCompressibility::CompressibleHistory,
                None,
            )?,
            LogicalConversationItem::Assistant {
                content, tool_calls, provider_history, ..
            } => {
                if let Some(history) = provider_history
                    && let Some(replay) = match request.provider {
                        Some(snapshot) => provider_history_replay(history, snapshot),
                        None => Some(std::borrow::Cow::Borrowed(&history.data)),
                    }
                {
                    let (text, opaque) = provider_reasoning_content(replay.as_ref());
                    push_text_budget_item(
                        &mut items,
                        wire_string(replay.as_ref())?,
                        estimate_native_reasoning(history.reasoning_tokens, &text, opaque),
                        ContextUsageKind::Conversation,
                        ContextSectionKind::RecentHistory,
                        ContextCompressibility::CompressibleHistory,
                        None,
                    )?;
                }
                if let Some(content) = content {
                    push_text_budget_item(
                        &mut items,
                        content.clone(),
                        BudgetTextKind::Text,
                        ContextUsageKind::Conversation,
                        ContextSectionKind::RecentHistory,
                        ContextCompressibility::CompressibleHistory,
                        None,
                    )?;
                }
                for call in tool_calls {
                    let content = match request.provider.zip(bindings.as_ref()) {
                        Some((snapshot, bindings)) => {
                            wire_string(&provider_tool_call(snapshot.api_format, call, bindings)?)?
                        }
                        None => canonical_string(call)?,
                    };
                    push_text_budget_item(
                        &mut items,
                        content,
                        BudgetTextKind::Text,
                        ContextUsageKind::ToolsAndResults,
                        ContextSectionKind::RecentHistory,
                        ContextCompressibility::CompressibleHistory,
                        None,
                    )?;
                }
            }
            LogicalConversationItem::ToolResult { content, .. } => push_text_budget_item(
                &mut items,
                content.clone(),
                BudgetTextKind::Text,
                ContextUsageKind::ToolsAndResults,
                ContextSectionKind::RecentHistory,
                ContextCompressibility::CompressibleHistory,
                None,
            )?,
        }
    }
    if request.split_user_sections
        && let Some(sections) = current_user_sections(assembly)?
    {
        for (index, (kind, content)) in sections.into_iter().enumerate() {
            let mut content = content.to_owned();
            if index < sections.len() - 1 {
                content.push('\n');
            }
            push_text_budget_item(
                &mut items,
                content,
                BudgetTextKind::Text,
                if kind == ContextSectionKind::FileReferencesAndPendingAttachments {
                    ContextUsageKind::BackgroundAndCompaction
                } else {
                    ContextUsageKind::Conversation
                },
                kind,
                budget_compressibility(assembly, kind)?,
                None,
            )?;
        }
    } else {
        for user in request.new_users {
            let LogicalConversationItem::User { content, .. } = user else {
                return Err(ContextPreparationError::ConversationInvalid);
            };
            push_text_budget_item(
                &mut items,
                content.clone(),
                BudgetTextKind::Text,
                ContextUsageKind::Conversation,
                ContextSectionKind::CurrentTurnWorkingSet,
                budget_compressibility(assembly, ContextSectionKind::CurrentTurnWorkingSet)?,
                None,
            )?;
        }
    }
    if let Some(question) = request.closing_question {
        push_text_budget_item(
            &mut items,
            question.to_owned(),
            BudgetTextKind::Text,
            ContextUsageKind::Conversation,
            ContextSectionKind::SteeredUserAdditions,
            ContextCompressibility::NeverCompress,
            None,
        )?;
    }
    for (index, attachment) in attachments.iter().enumerate() {
        let ordinal = next_ordinal(&items)?;
        items.push(PendingBudgetItem {
            ordinal,
            payload: PendingBudgetPayload::NativeAttachment(attachment.clone()),
            assignment: BudgetAssignment {
                usage: ContextUsageKind::BackgroundAndCompaction,
                section: ContextSectionKind::FileReferencesAndPendingAttachments,
                compressibility: ContextCompressibility::UserReducibleOnly,
                tool_index: None,
                attachment_index: Some(index),
            },
        });
    }
    Ok(items)
}

fn canonical_string(value: &impl serde::Serialize) -> Result<String, ContextPreparationError> {
    let bytes =
        canonical_json_v1(value).map_err(|_| ContextPreparationError::CanonicalSerialization)?;
    String::from_utf8(bytes).map_err(|_| ContextPreparationError::CanonicalSerialization)
}

fn wire_string(value: &impl serde::Serialize) -> Result<String, ContextPreparationError> {
    serde_json::to_string(value).map_err(|_| ContextPreparationError::CanonicalSerialization)
}

fn push_text_budget_item(
    items: &mut Vec<PendingBudgetItem>,
    content: String,
    kind: BudgetTextKind,
    usage: ContextUsageKind,
    section: ContextSectionKind,
    compressibility: ContextCompressibility,
    tool_index: Option<usize>,
) -> Result<(), ContextPreparationError> {
    let ordinal = next_ordinal(items)?;
    items.push(PendingBudgetItem {
        ordinal,
        payload: PendingBudgetPayload::Text(content, kind),
        assignment: BudgetAssignment {
            usage,
            section,
            compressibility,
            tool_index,
            attachment_index: None,
        },
    });
    Ok(())
}

fn next_ordinal(items: &[PendingBudgetItem]) -> Result<u32, ContextPreparationError> {
    u32::try_from(items.len() + 1).map_err(|_| ContextPreparationError::ArithmeticOverflow)
}

pub(super) fn estimator_inputs(
    items: &[PendingBudgetItem],
) -> Result<(Vec<CanonicalTextBudgetItem>, Vec<NativeAttachmentBudgetItem>), ContextPreparationError>
{
    let mut text = Vec::new();
    let mut native = Vec::new();
    for item in items {
        match &item.payload {
            PendingBudgetPayload::Text(content, kind) => text.push(CanonicalTextBudgetItem {
                ordinal: item.ordinal,
                content_hash: ContentHash::sha256(content.as_bytes()),
                canonical_text: content.clone(),
                kind: *kind,
            }),
            PendingBudgetPayload::NativeAttachment(attachment) => {
                native.push(NativeAttachmentBudgetItem {
                    ordinal: item.ordinal,
                    attachment: attachment.clone(),
                });
            }
        }
    }
    Ok((text, native))
}

#[derive(Clone, Debug)]
pub(super) struct ContextAccounting {
    pub(super) usage: ContextUsageSummaryView,
    pub(super) section_tokens: BTreeMap<ContextSectionKind, u64>,
    pub(super) tool_tokens: Vec<u64>,
    pub(super) attachment_tokens: Vec<u64>,
    pub(super) non_compressible_tokens: u64,
    pub(super) compressible_tokens: u64,
    pub(super) user_reducible_tokens: u64,
}

pub(super) fn build_accounting(
    pending: &[PendingBudgetItem],
    budget: &ContextBudgetSnapshot,
) -> Result<ContextAccounting, ContextPreparationError> {
    let estimates = budget
        .item_estimates
        .iter()
        .map(|estimate| (estimate.ordinal, estimate))
        .collect::<BTreeMap<_, _>>();
    let mut usage_tokens = BTreeMap::<ContextUsageKind, u64>::new();
    let mut section_tokens = BTreeMap::<ContextSectionKind, u64>::new();
    let tool_count = pending
        .iter()
        .filter_map(|item| item.assignment.tool_index)
        .max()
        .map_or(0, |index| index + 1);
    let attachment_count = pending
        .iter()
        .filter_map(|item| item.assignment.attachment_index)
        .max()
        .map_or(0, |index| index + 1);
    let mut tool_tokens = vec![0_u64; tool_count];
    let mut attachment_tokens = vec![0_u64; attachment_count];
    let mut non_compressible_tokens = 0_u64;
    let mut compressible_tokens = 0_u64;
    let mut user_reducible_tokens = 0_u64;
    for item in pending {
        let tokens = estimates
            .get(&item.ordinal)
            .ok_or(ContextPreparationError::BudgetInvalid)?
            .estimated_tokens;
        checked_add_map(&mut usage_tokens, item.assignment.usage, tokens)?;
        checked_add_map(&mut section_tokens, item.assignment.section, tokens)?;
        if let Some(index) = item.assignment.tool_index {
            tool_tokens[index] = tool_tokens[index]
                .checked_add(tokens)
                .ok_or(ContextPreparationError::ArithmeticOverflow)?;
        }
        if let Some(index) = item.assignment.attachment_index {
            attachment_tokens[index] = attachment_tokens[index]
                .checked_add(tokens)
                .ok_or(ContextPreparationError::ArithmeticOverflow)?;
        }
        match item.assignment.compressibility {
            ContextCompressibility::NeverCompress => {
                non_compressible_tokens = non_compressible_tokens
                    .checked_add(tokens)
                    .ok_or(ContextPreparationError::ArithmeticOverflow)?;
            }
            ContextCompressibility::CompressibleHistory
            | ContextCompressibility::CompressibleSealedActiveSegment => {
                compressible_tokens = compressible_tokens
                    .checked_add(tokens)
                    .ok_or(ContextPreparationError::ArithmeticOverflow)?;
            }
            ContextCompressibility::UserReducibleOnly => {
                user_reducible_tokens = user_reducible_tokens
                    .checked_add(tokens)
                    .ok_or(ContextPreparationError::ArithmeticOverflow)?;
            }
        }
    }
    let estimated = budget.item_estimates.iter().any(|estimate| {
        estimate.estimator.exactness != aura_context_domain::TokenEstimateExactness::Exact
    });
    let segments = ContextUsageKind::ORDER
        .into_iter()
        .map(|kind| ContextUsageSegmentView {
            kind,
            label: usage_label(kind).to_owned(),
            estimated_tokens: usage_tokens.get(&kind).copied().unwrap_or_default(),
        })
        .collect();
    let usage = ContextUsageSummaryView {
        runtime_effective_window_tokens: budget.runtime_effective_window_tokens,
        used_input_tokens: budget.estimated_input_tokens,
        estimated,
        segments,
    };
    if !usage.is_consistent() {
        return Err(ContextPreparationError::BudgetInvalid);
    }
    Ok(ContextAccounting {
        usage,
        section_tokens,
        tool_tokens,
        attachment_tokens,
        non_compressible_tokens,
        compressible_tokens,
        user_reducible_tokens,
    })
}

fn checked_add_map<K: Ord>(
    values: &mut BTreeMap<K, u64>,
    key: K,
    value: u64,
) -> Result<(), ContextPreparationError> {
    let entry = values.entry(key).or_default();
    *entry = entry.checked_add(value).ok_or(ContextPreparationError::ArithmeticOverflow)?;
    Ok(())
}

fn budget_compressibility(
    assembly: &ContextSectionAssembly,
    kind: ContextSectionKind,
) -> Result<ContextCompressibility, ContextPreparationError> {
    assembly
        .sections
        .iter()
        .find(|section| section.kind == kind)
        .map(|section| {
            if section.is_empty_marker {
                ContextCompressibility::NeverCompress
            } else {
                section.compressibility
            }
        })
        .ok_or(ContextPreparationError::SectionsInvalid)
}

const fn instruction_section(
    kind: aura_model_domain::LogicalInstructionKind,
) -> ContextSectionKind {
    match kind {
        aura_model_domain::LogicalInstructionKind::Core => ContextSectionKind::CoreSystemPrompt,
        aura_model_domain::LogicalInstructionKind::Permission => {
            ContextSectionKind::PermissionAndSecurity
        }
        aura_model_domain::LogicalInstructionKind::ToolSummary => {
            ContextSectionKind::LoadedToolSchemas
        }
        aura_model_domain::LogicalInstructionKind::WorkspaceAgents => {
            ContextSectionKind::WorkspaceAgentsInstructions
        }
        aura_model_domain::LogicalInstructionKind::HistoricalSnapshot => {
            ContextSectionKind::HistoricalSnapshotSummary
        }
        aura_model_domain::LogicalInstructionKind::AgentMode => ContextSectionKind::AgentModePrompt,
        aura_model_domain::LogicalInstructionKind::Hook => ContextSectionKind::HookInjections,
        aura_model_domain::LogicalInstructionKind::TaskState => {
            ContextSectionKind::CurrentTaskState
        }
    }
}

const fn instruction_usage(kind: aura_model_domain::LogicalInstructionKind) -> ContextUsageKind {
    match kind {
        aura_model_domain::LogicalInstructionKind::Core
        | aura_model_domain::LogicalInstructionKind::Permission
        | aura_model_domain::LogicalInstructionKind::WorkspaceAgents
        | aura_model_domain::LogicalInstructionKind::AgentMode
        | aura_model_domain::LogicalInstructionKind::Hook => ContextUsageKind::SystemAndRules,
        aura_model_domain::LogicalInstructionKind::ToolSummary => ContextUsageKind::ToolsAndResults,
        aura_model_domain::LogicalInstructionKind::HistoricalSnapshot => {
            ContextUsageKind::BackgroundAndCompaction
        }
        aura_model_domain::LogicalInstructionKind::TaskState => ContextUsageKind::Conversation,
    }
}

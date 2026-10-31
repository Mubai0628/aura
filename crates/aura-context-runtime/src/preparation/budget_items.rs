//! The parts a request's budget counts, in the order they are sent, each with the section and the
//! use it is shown under.
use super::*;
use crate::{BudgetTextKind, CanonicalTextBudgetItem, NativeAttachmentBudgetItem};

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

pub(super) fn build_pending_budget_items(
    assembly: &ContextSectionAssembly,
    schema_set: &ToolSchemaSetSnapshotV1,
    tools: &[ContextToolInput],
    recent_history: &[LogicalConversationItem],
    attachments: &[ModelAttachmentSnapshot],
) -> Result<Vec<PendingBudgetItem>, ContextPreparationError> {
    let mut items = Vec::new();
    for (index, block) in assembly.instruction_envelope.ordered_blocks.iter().enumerate() {
        let section = instruction_section(block.kind);
        let mut content = block.content.clone();
        if index + 1 < assembly.instruction_envelope.ordered_blocks.len() {
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
    for (index, (tool, schema)) in tools.iter().zip(&schema_set.tools).enumerate() {
        let bytes = canonical_json_v1(schema)
            .map_err(|_| ContextPreparationError::CanonicalSerialization)?;
        let content = String::from_utf8(bytes)
            .map_err(|_| ContextPreparationError::CanonicalSerialization)?;
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
    for item in recent_history {
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
                if let Some(history) = provider_history {
                    let history = serde_json::to_string(history)
                        .map_err(|_| ContextPreparationError::CanonicalSerialization)?;
                    push_text_budget_item(
                        &mut items,
                        history,
                        BudgetTextKind::ProviderHistory,
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
                    let bytes = canonical_json_v1(call)
                        .map_err(|_| ContextPreparationError::CanonicalSerialization)?;
                    push_text_budget_item(
                        &mut items,
                        String::from_utf8(bytes)
                            .map_err(|_| ContextPreparationError::CanonicalSerialization)?,
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
    let current_kinds = [
        ContextSectionKind::FileReferencesAndPendingAttachments,
        ContextSectionKind::CurrentTurnWorkingSet,
        ContextSectionKind::SteeredUserAdditions,
    ];
    for (index, kind) in current_kinds.into_iter().enumerate() {
        let section = assembly
            .sections
            .iter()
            .find(|section| section.kind == kind)
            .ok_or(ContextPreparationError::SectionsInvalid)?;
        let mut content = section.canonical_content.clone();
        if index + 1 < current_kinds.len() {
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
            if section.is_empty_marker {
                ContextCompressibility::NeverCompress
            } else {
                section.compressibility
            },
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

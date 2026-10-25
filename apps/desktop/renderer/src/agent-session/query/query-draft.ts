import type { QueryAnswerValue, QueryQuestion } from '@aura/contracts/generated';

/** What the user has given one question so far. */
export interface QuestionDraft {
  /** The answer in the question's own form: a choice, a confirmation, text or a file. */
  value?: QueryAnswerValue | undefined;
  /** Every question takes the user's own words instead. */
  custom: boolean;
  customText: string;
}

export const MAX_CUSTOM_REPLY_CHARS = 4000;

/** The draft as an answer, or why it is not one yet. */
export function draftAnswer(
  question: QueryQuestion,
  draft: QuestionDraft,
): QueryAnswerValue | string {
  if (draft.custom) {
    const text = draft.customText.trim();
    return text ? { answer: 'custom', text } : '写下你的自定义回复。';
  }
  const value = draft.value;
  if (!value) return question.answer_type.type === 'text' ? '写下你的回答。' : '选择一个回复。';
  if (value.answer === 'choices' && value.ids.length === 0) return '至少选择一个回复。';
  if (value.answer === 'text' && !value.text.trim()) return '写下你的回答。';
  return value;
}

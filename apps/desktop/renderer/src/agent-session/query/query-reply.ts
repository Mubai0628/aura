/**
 * What an answer to the model's question says, read from the reply the model
 * was sent, so the conversation shows the questions and the replies — never the raw JSON
 * ("用户信息 [{"question_id":…}]"), and also when the user declined or nobody responded.
 */
interface ReplyEntry {
  question: string;
  answer: string;
  custom: boolean;
}

export interface QueryReplyView {
  outcome: 'answered' | 'declined' | 'no_response';
  entries: ReplyEntry[];
}

type Json = Record<string, unknown>;
const record = (value: unknown): Json | undefined =>
  typeof value === 'object' && value !== null && !Array.isArray(value)
    ? (value as Json)
    : undefined;
const text = (value: unknown) => (typeof value === 'string' ? value : '');

function replyText(reply: Json | undefined): { answer: string; custom: boolean } {
  switch (reply?.kind) {
    case 'choices': {
      const chosen = Array.isArray(reply.chosen) ? reply.chosen : [];
      const labels = chosen.map((item) => text(record(item)?.label) || text(record(item)?.id));
      return { answer: labels.join('；'), custom: false };
    }
    case 'text':
      return { answer: text(reply.text), custom: false };
    case 'custom':
      return { answer: text(reply.text), custom: true };
    case 'confirm':
      return { answer: reply.accepted === true ? '确认' : '不确认', custom: false };
    case 'file_or_directory':
      return { answer: text(reply.relative_path) || '已选择工作区里的文件或目录', custom: false };
    case 'declined':
      return { answer: '拒绝回答', custom: false };
    case 'no_response':
      return { answer: '无人回应', custom: false };
    default:
      return { answer: '', custom: false };
  }
}

/** What an answer message says, or undefined when it is not one this page can read. */
export function parseQueryReply(message: string): QueryReplyView | undefined {
  let value: unknown;
  try {
    value = JSON.parse(message);
  } catch {
    return undefined;
  }
  const body = record(record(value)?.query_reply);
  if (body) {
    const outcome = body.outcome;
    if (outcome !== 'answered' && outcome !== 'declined' && outcome !== 'no_response')
      return undefined;
    const answers = Array.isArray(body.answers) ? body.answers : [];
    return {
      outcome,
      entries: answers.map((item) => {
        const entry = record(item);
        return { question: text(entry?.question), ...replyText(record(entry?.reply)) };
      }),
    };
  }
  // Answers written by older versions carry ids only.
  const legacy = Array.isArray(value) ? value : record(value)?.answers;
  if (!Array.isArray(legacy)) return undefined;
  return {
    outcome: 'answered',
    entries: legacy.map((item) => {
      const entry = record(item);
      const answer = record(entry?.value);
      const shown =
        answer?.answer === 'choices' && Array.isArray(answer.ids)
          ? answer.ids.map(text).join('；')
          : answer?.answer === 'confirm'
            ? answer.accepted === true
              ? '确认'
              : '不确认'
            : text(answer?.text) || '已回答';
      return { question: text(entry?.question_id), answer: shown, custom: false };
    }),
  };
}

import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, test } from 'vitest';
import { parseQueryReply } from './query-reply';
import { QueryReplyCard } from './QueryReplyCard';

// The answer showed as "用户信息 [{"question_id":…}]".

afterEach(cleanup);

const reply = (body: object) => JSON.stringify({ query_reply: body });

test('an answer reads as the questions and the replies, the user’s own words marked', () => {
  const view = parseQueryReply(
    reply({
      query_id: 'qry_1',
      outcome: 'answered',
      answers: [
        {
          question_id: 'empty_workspace',
          question: '工作区是空的，你希望我怎么处理？',
          reply: { kind: 'choices', chosen: [{ id: 'minimal', label: '创建最小 AGENTS.md' }] },
        },
        {
          question_id: 'proceed',
          question: '现在继续吗？',
          reply: { kind: 'custom', text: '等我看完' },
        },
      ],
    }),
  );
  render(<QueryReplyCard view={view!} />);
  const card = screen.getByRole('article');
  expect(card.textContent).toContain('你的回答');
  expect(card.textContent).toContain('工作区是空的，你希望我怎么处理？创建最小 AGENTS.md');
  expect(card.textContent).toContain('现在继续吗？自定义等我看完');
  expect(card.textContent).not.toContain('question_id');
});

test('a declined question and one nobody touched say what the model was told', () => {
  for (const [outcome, heading, words] of [
    ['declined', '你拒绝了回答', '不要再追问'],
    ['no_response', '提问无人回应（已超时）', '先做不受影响的工作'],
  ] as const) {
    const view = parseQueryReply(
      reply({
        query_id: 'qry_1',
        outcome,
        note: 'for the model',
        answers: [{ question_id: 'q', question: '继续吗？', reply: { kind: outcome } }],
      }),
    );
    const { unmount } = render(<QueryReplyCard view={view!} />);
    const text = screen.getByRole('article').textContent ?? '';
    expect(text).toContain(heading);
    expect(text).toContain(words);
    expect(text).not.toContain('for the model');
    unmount();
  }
});

test('an answer written before L10.14 still reads without JSON, and a cut preview is left alone', () => {
  const legacy = parseQueryReply(
    JSON.stringify([
      { question_id: 'empty_workspace', value: { answer: 'choices', ids: ['minimal'] } },
    ]),
  );
  expect(legacy).toEqual({
    outcome: 'answered',
    entries: [{ question: 'empty_workspace', answer: 'minimal', custom: false }],
  });
  expect(parseQueryReply('{"query_reply":{"query_id":"qry_1","outcome":"answ')).toBeUndefined();
});

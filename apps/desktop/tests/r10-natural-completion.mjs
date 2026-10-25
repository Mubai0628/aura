/* global document, window */
// This batch in one real window: a Normal turn ends from its own final answer
// (no completion Tool call), that answer is rendered once, consecutive same-Tool
// calls form one group, every Tool row keeps its icon slot, and the recent-activity
// card follows the measured window width. Uses the real dev namespace, the
// already configured non-Pro DeepSeek Flash profile and the real UI only.
import assert from 'node:assert/strict';
import { join } from 'node:path';
import {
  api,
  completed,
  harness,
  observeIncremental,
  poll,
  sendNew,
  sendNext,
} from './r10-session-harness.mjs';

const h = await harness('r10-natural-completion', { realData: true });
let failure;
try {
  const settings = await api(h.page, 'getModelSettings');
  const configured = new Set(
    (settings.active_credentials ?? [])
      .filter((c) => c.state === 'configured')
      .map((c) => c.endpoint_id),
  );
  const candidates = Object.values(settings.registry.models).filter(
    (m) =>
      /^deepseek.*flash/iu.test(m.provider_model_name) &&
      !/pro/iu.test(m.provider_model_name) &&
      m.enabled &&
      configured.has(m.endpoint_id),
  );
  const model = candidates.find((m) => m.provider_model_name === 'deepseek-flash') ?? candidates[0];
  assert.ok(model, 'Configured non-Pro DeepSeek Flash is required; credentials are not modified');
  h.record('real model', {
    name: model.provider_model_name,
    displayName: model.display_name,
    proExcluded: true,
  });

  const session = await sendNew(
    h,
    model.display_name,
    '请直接用一句中文回答：整理工作区文件时应该先做什么？' +
      '这一轮只回复文字，不要调用任何工具（包括完成或收尾类工具），也不要读写文件。',
  );
  await observeIncremental(h.page, session);
  const view = await completed(h, session, 1, 300_000);
  const turn = view.turns.at(-1);
  const turnId = turn.turn.turn_task_id;
  const scope = h.page.locator(`[data-turn-id="${turnId}"]`);

  // 1. The final answer is the answering ModelStep's own response.
  const source = turn.final_text_source;
  assert.equal(source?.kind, 'assistant_step', 'the answering step is the final text source');
  const answered = turn.assistant_messages.find((m) => m.model_step_id === source.model_step_id);
  assert.ok(answered, 'the answering step is part of the real projection');
  assert.equal(
    answered.tool_call_ids.length,
    0,
    'a natural completion adds no completion Tool call to the answer',
  );
  // A single ModelStep means the turn ended on that response: no reminder round trip.
  assert.equal(
    turn.assistant_messages.length,
    1,
    'the text-only turn asked the real model exactly once',
  );
  h.record('natural completion', {
    turnId,
    finalTextSource: source.kind,
    modelStep: source.model_step_id,
    assistantSteps: turn.assistant_messages.length,
    answeredStepHasNoToolCall: true,
  });

  // 2. The answer appears once: the trace keeps Reasoning, Tools and results.
  const finalArea = scope.locator('.agent-final-report');
  assert.equal(await finalArea.count(), 1, 'exactly one final area');
  const finalText = (await finalArea.innerText()).trim();
  assert.ok(finalText.length > 0, 'the real answer is rendered');
  await scope.locator('.agent-process-toggle').click();
  await scope.locator(`.agent-process-step[data-step-id="${source.model_step_id}"]`).waitFor();
  assert.equal(
    await scope
      .locator(`.agent-process-step[data-step-id="${source.model_step_id}"] .agent-process-text`)
      .count(),
    0,
    'the answering step contributes no second copy of the final text',
  );
  const traceTexts = await scope.locator('.agent-process-text').allInnerTexts();
  assert.ok(
    !traceTexts.some((text) => text.trim() === finalText),
    'the final answer is not repeated anywhere in the trace',
  );
  h.record('final text rendered once', {
    characters: [...finalText].length,
    traceSteps: traceTexts.length,
    duplicatedInTrace: false,
  });

  // 3. A real tool turn: consecutive same-Tool calls in one step.
  await sendNext(
    h,
    '请在同一个回复里连续调用两次读取文件工具：先读取 note.txt，再读取 中文文件.txt，' +
      '然后用一句中文说明两个文件各自的合成标记。不要写入或修改任何文件，也不要运行命令。',
  );
  const toolView = await completed(h, session, 2, 300_000);
  const toolTurn = toolView.turns.at(-1);
  const toolTurnId = toolTurn.turn.turn_task_id;
  const toolScope = h.page.locator(`[data-turn-id="${toolTurnId}"]`);
  await toolScope.locator('.agent-process-toggle').click();

  // 4. Real consecutive same-Tool calls and the icons of every real row.
  const answerIds = toolTurn.assistant_messages.flatMap((m) => m.tool_call_ids);
  const activity = await api(h.page, 'readAgentToolActivity', {
    session_id: session,
    before_call_id: null,
    call_ids: answerIds,
    limit: 100,
  });
  const visible = activity.calls.filter(
    (call) => !call.display.internal && answerIds.includes(call.summary.tool_call_id),
  );
  const canonical = (call) => call?.summary?.display_name ?? call?.display_name ?? null;
  const key = (call) =>
    `${canonical(call)}#${call.summary.outcome?.type ?? `status:${call.summary.status}`}`;
  const runs = (ids) => {
    const result = [];
    for (const id of ids) {
      const call = visible.find((item) => item.summary.tool_call_id === id);
      if (!call) continue;
      const previous = result.at(-1);
      if (previous && previous.key === key(call)) previous.ids.push(id);
      else result.push({ key: key(call), ids: [id] });
    }
    return result;
  };
  const stepRuns = toolTurn.assistant_messages.flatMap((message) =>
    runs(message.tool_call_ids.filter((id) => visible.some((c) => c.summary.tool_call_id === id))),
  );
  const grouped = await toolScope.locator('.agent-tool-group').count();
  const rows = await toolScope.locator('.agent-tool-activity').count();
  assert.equal(rows, visible.length, 'every real Tool call keeps its own row');
  const multi = stepRuns.filter((run) => run.ids.length > 1);
  if (multi.length) assert.ok(grouped >= 1, 'a real consecutive same-Tool run forms a group');
  for (let index = 0; index < grouped; index += 1) {
    const group = toolScope.locator('.agent-tool-group').nth(index);
    // The header lives in the summary; expanded members carry their own state text.
    const state = (await group.locator('summary .agent-tool-state').innerText()).trim();
    assert.match(state, /·\s*\d+\s*次/u, 'the group header states the real call count');
    const count = Number(/(\d+)\s*次/u.exec(state)?.[1]);
    // Expansion keeps every call readable: targets, identity and result survive.
    await group.locator('summary').click();
    const members = group.locator('.agent-tool-group-members .agent-tool-activity');
    assert.equal(await members.count(), count, 'the group expands to exactly its own calls');
    const titles = await members.locator('.agent-tool-title').allInnerTexts();
    assert.equal(
      new Set(titles.map((title) => title.trim())).size,
      1,
      'a group never merges different Tools',
    );
    for (let member = 0; member < count; member += 1)
      assert.ok(
        await members.nth(member).locator('.agent-tool-kind').first().getAttribute('data-icon'),
        'every expanded member keeps its Tool-kind icon',
      );
  }
  const groupedCalls = stepRuns
    .filter((run) => run.ids.length > 1)
    .reduce((total, run) => total + run.ids.length, 0);
  const expectedGroups = multi.length;
  assert.equal(grouped, expectedGroups, 'grouping matches the real consecutive runs');
  const toggleText = await toolScope.locator('.agent-process-toggle').innerText();
  assert.match(
    toggleText,
    new RegExp(`${visible.length} 次工具调用`, 'u'),
    'the trace still reports the real call count',
  );

  // Every row carries its Tool-kind icon next to text, plus a state icon. Rows are
  // rendered in projection order, so a row can be matched to its real call.
  const renderedOrder = toolTurn.assistant_messages.flatMap((message) =>
    message.tool_call_ids.filter((id) => visible.some((c) => c.summary.tool_call_id === id)),
  );
  assert.equal(renderedOrder.length, rows, 'every visible call is rendered in order');
  const iconFor = (name) =>
    ['fs.read', 'fs.read_text'].includes(name)
      ? 'file-text'
      : name === 'fs.list' || name === 'fs.glob'
        ? 'folder-tree'
        : name.startsWith('shell.')
          ? 'terminal'
          : null;
  const iconByCall = [];
  for (let index = 0; index < rows; index += 1) {
    const row = toolScope.locator('.agent-tool-activity').nth(index);
    const call = visible.find((item) => item.summary.tool_call_id === renderedOrder[index]);
    const kind = await row.locator('.agent-tool-kind').first().getAttribute('data-icon');
    assert.ok(kind, 'a Tool row keeps a Tool-kind icon');
    const expected = iconFor(canonical(call) ?? '');
    if (expected) {
      assert.equal(kind, expected, `${canonical(call)} keeps the icon of its canonical Tool name`);
    }
    iconByCall.push({
      name: canonical(call),
      title: call?.display?.title ?? null,
      target: call?.display?.target ?? null,
      icon: kind,
    });
    const stateText = (await row.locator('.agent-tool-state').innerText()).trim();
    assert.ok(stateText.length > 0, 'a Tool row keeps real state text');
    const stateIcon = await row
      .locator('.agent-tool-state svg[data-icon]')
      .first()
      .getAttribute('data-icon');
    assert.ok(stateIcon, 'state text is paired with a state icon');
  }
  const distinctIcons = new Set(iconByCall.map((entry) => `${entry.name ?? '?'}#${entry.icon}`));
  assert.equal(
    distinctIcons.size,
    new Set(iconByCall.map((entry) => entry.name ?? '?')).size,
    'one canonical Tool name always keeps one icon',
  );
  const noteTargets = iconByCall.filter((entry) => String(entry.target ?? '').includes('note.txt'));
  const groupedExpanded = grouped > 0;
  h.record('grouping and icons', {
    realCalls: visible.length,
    groups: grouped,
    groupedCalls,
    multiCallRuns: multi.length,
    expandedGroup: groupedExpanded,
    iconsVerified: true,
    calls: iconByCall,
    noteTargetMissing: noteTargets.length === 0,
  });

  // 4. The recent-activity card follows the measured window, not a fixed width.
  // With both sidebars open the authored crowding rule keeps the card compact at
  // every width, so the real check is the compact entry: it opens the complete
  // card inside the window, Escape closes it and returns focus.
  const card = h.page.locator('.activity-stack');
  const nativeWindow = await h.app.browserWindow(h.page);
  // A maximized window ignores setSize, so each step unmaximizes first and waits
  // for the renderer to report the applied content width.
  const resize = (width, height) =>
    nativeWindow.evaluate(
      (window, size) => {
        if (window.isMaximized()) window.unmaximize();
        window.setSize(size.width, size.height);
      },
      { width, height },
    );
  const cardState = () =>
    card.evaluate((node) => ({
      display: node.getAttribute('data-display'),
      expanded: node.getAttribute('data-expanded'),
      width: Math.round(node.getBoundingClientRect().width),
      innerWidth: window.innerWidth,
      // The same measurement the card uses: space right of the conversation column.
      available: Math.round(
        document.querySelector('.conversation-canvas').getBoundingClientRect().right -
          12 -
          document.querySelector('.conversation-column').getBoundingClientRect().right,
      ),
    }));
  /** The compact entry must show its whole mode word, not a clipped slot. */
  const entryGeometry = () =>
    h.page.evaluate(() => {
      const entry = document.querySelector('.activity-compact-entry');
      if (entry === null) return null;
      const mode = entry.querySelector('.activity-compact-mode');
      const entryBox = entry.getBoundingClientRect();
      const modeBox = mode.getBoundingClientRect();
      return {
        label: entry.getAttribute('aria-label'),
        text: mode.textContent?.trim() ?? '',
        boxWidth: Math.round(entryBox.width),
        clipping: entry.scrollWidth - entry.clientWidth,
        textInside: modeBox.left >= entryBox.left - 0.5 && modeBox.right <= entryBox.right + 0.5,
        textClipped: mode.scrollWidth - mode.clientWidth,
      };
    });
  // The widest window the display allows: this is the only real chance for the
  // full card, because the conversation column keeps most of the width.
  await resize(1900, 1000);
  const widest = await poll(
    () => cardState(),
    (value) => value.innerWidth > 1400 && ['full', 'compact'].includes(value.display),
    15_000,
    'the widest window is applied and reports its measured card display',
  );
  let widestFull = null;
  if (widest.display === 'full') {
    widestFull = await card.evaluate((node) => {
      const box = node.getBoundingClientRect();
      return {
        x: Math.round(box.x),
        width: Math.round(box.width),
        right: Math.round(box.right),
        columnRight: Math.round(
          document.querySelector('.conversation-column').getBoundingClientRect().right,
        ),
      };
    });
    assert.ok(
      widestFull.width >= 280 && widestFull.width <= 320,
      'the full card uses its authored width range',
    );
    assert.ok(widestFull.x >= widestFull.columnRight - 1, 'the full card never covers the column');
    await h.page.screenshot({ path: join(h.output, 'widest-full-card.png') });
  }
  await resize(1720, 980);
  const wide = await poll(
    () => cardState(),
    (value) => value.innerWidth > 1200 && ['full', 'compact'].includes(value.display),
    15_000,
    'the wide window is applied and reports its measured card display',
  );
  await h.page.screenshot({ path: join(h.output, 'wide-card.png') });
  if (wide.display === 'full') {
    assert.ok(
      wide.width >= 280 && wide.width <= 320,
      'a full card stays inside its authored range',
    );
    // A full card lives in the measured space right of the conversation column.
    const fullBox = await card.evaluate((node) => {
      const box = node.getBoundingClientRect();
      return {
        x: Math.round(box.x),
        columnRight: Math.round(
          document.querySelector('.conversation-column').getBoundingClientRect().right,
        ),
      };
    });
    assert.ok(fullBox.x >= fullBox.columnRight - 1, 'a full card never covers the column');
  }

  await resize(1040, 900);
  const narrow = await poll(
    () => cardState(),
    (value) => value.innerWidth <= 1200 && ['full', 'compact'].includes(value.display),
    15_000,
    'the narrow window is applied and reports its measured card display',
  );
  const entry = h.page.locator('.activity-compact-entry');
  if (narrow.display === 'compact') {
    const entryLabel = await entry.getAttribute('aria-label');
    assert.match(entryLabel ?? '', /展开近期活动/u, 'the compact entry names the real mode');
    assert.equal(narrow.expanded, 'false');
    // The whole mode word must be inside the pill: the icon-button 28px slot must
    // not clip it.
    const pill = await entryGeometry();
    assert.ok(pill, 'the compact entry is rendered');
    assert.ok(['Normal', 'Plan', 'Goal'].includes(pill.text), `mode text: ${pill.text}`);
    assert.ok(pill.boxWidth > 28, `the pill sizes to its label: ${pill.boxWidth}px`);
    assert.equal(pill.clipping, 0, 'the pill does not clip its own content');
    assert.equal(pill.textClipped, 0, 'the mode word is not clipped');
    assert.equal(pill.textInside, true, 'the mode word is inside the pill');
    await h.page.screenshot({ path: join(h.output, 'narrow-compact-entry.png') });

    await entry.click();
    await poll(
      () => card.getAttribute('data-expanded'),
      (value) => value === 'true',
      10_000,
      'the compact entry expands the complete card in the window',
    );
    // One control at a time: the entry is gone while the card is open.
    assert.equal(
      await h.page.getByRole('button', { name: /展开近期活动/u }).count(),
      0,
      'the expanded card leaves exactly one close control',
    );
    assert.equal(
      await h.page.getByRole('button', { name: '收起卡片', exact: true }).count(),
      1,
      'the card carries the close control',
    );
    const cardBox = await poll(
      () =>
        card.evaluate((node) => {
          const box = node.getBoundingClientRect();
          return {
            x: Math.round(box.x),
            right: Math.round(box.right),
            width: Math.round(box.width),
            innerWidth: window.innerWidth,
          };
        }),
      // The card detaches with a short animation: measure after it settles.
      (value) => value.x >= -1 && value.right <= value.innerWidth + 1,
      5_000,
      'the expanded card settles inside the window',
    );
    await h.page.screenshot({ path: join(h.output, 'narrow-expanded-card.png') });

    await h.page.keyboard.press('Escape');
    await poll(
      () => card.getAttribute('data-expanded'),
      (value) => value === 'false',
      10_000,
      'Escape collapses the card again',
    );
    const focused = await h.page.evaluate(() => document.activeElement?.getAttribute('aria-label'));
    assert.match(focused ?? '', /展开近期活动/u, 'focus returns to the entry that opened the card');
    h.record('recent activity card', {
      widest,
      widestFull,
      wide,
      narrow,
      fullBranch: [widest, wide, narrow].some((value) => value.display === 'full')
        ? 'executed'
        : 'not-executed',
      compactEntry: entryLabel,
      compactEntryBox: pill,
      expandedBox: cardBox,
      singleCloseControl: true,
      expandedInWindow: true,
      escapeRestoresFocus: true,
      screenshots: ['wide-card.png', 'narrow-compact-entry.png', 'narrow-expanded-card.png'],
    });
  } else {
    // The measured space was enough for the full card at both sizes.
    assert.ok(
      narrow.width >= 280 && narrow.width <= 320,
      'a full card stays inside its authored range',
    );
    await h.page.screenshot({ path: join(h.output, 'narrow-full-card.png') });
    h.record('recent activity card', {
      wide,
      narrow,
      fullBranch: 'executed',
      compactEntry: null,
      expandedInWindow: false,
      escapeRestoresFocus: false,
      screenshots: ['wide-card.png', 'narrow-full-card.png'],
    });
  }

  // 5. The adaptive branch on the real screen. The card follows the measured space
  // right of the conversation column, so this records that measurement and tries
  // the one real control that frees space: closing the right sidebar.
  const adaptive = {
    rightSidebarControl: null,
    measuredAvailable: narrow.available,
    switchedToFull: false,
    returnedToCompact: false,
  };
  const closeRight = h.page.getByRole('button', { name: '关闭右侧边栏', exact: true });
  const openRight = h.page.getByRole('button', { name: '展开右侧边栏', exact: true });
  if (await closeRight.count()) {
    adaptive.rightSidebarControl = '关闭右侧边栏';
    await closeRight.first().click();
    const freed = await poll(
      () => cardState(),
      (value) => ['full', 'compact'].includes(value.display),
      10_000,
      'the card reports its display after the right sidebar closed',
    );
    adaptive.freedAvailable = freed.available;
    if (freed.display === 'full') {
      assert.ok(
        freed.width >= 280 && freed.width <= 320,
        'the freed window shows the authored full card',
      );
      assert.equal(freed.expanded, 'false', 'a full card needs no compact entry');
      await h.page.screenshot({ path: join(h.output, 'wide-full-card.png') });
      adaptive.switchedToFull = true;
    }
    if (await openRight.count()) {
      await openRight.first().click();
      const restored = await poll(
        () => cardState(),
        (value) => value.display === 'compact',
        10_000,
        'the card returns to the compact entry',
      );
      adaptive.returnedToCompact = restored.display === 'compact';
    }
  } else if (await openRight.count()) {
    // No tool panel is open, so the sidebar is already out of the way: the real
    // window simply has too little space right of the conversation column.
    adaptive.rightSidebarControl = '展开右侧边栏';
  }
  h.record('adaptive card branch', adaptive);

  await resize(1720, 980);
  Object.assign(h.report, {
    finalTextCharacters: [...finalText].length,
    processToggle: toggleText,
  });
} catch (error) {
  failure = error;
}
await h.finish(failure);

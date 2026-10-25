import { describe, expect, it, vi } from 'vitest';

import {
  CONTROL_UNRESOLVED_TERMINAL_CAPACITY,
  isProvablyUnsent,
  MAX_ACTIVE_REQUESTS,
  MAX_CONTROL_REQUESTS,
  RequestAdmissionError,
  RequestRegistry,
  RETIRED_CAPACITY,
  RETIRED_TTL_MS,
  UNRESOLVED_TERMINAL_CAPACITY,
  type AdmissionRefusal,
  type RegistryLimits,
} from './request-registry.js';

/**
 * The retention window is the deliberate boundary of "we remember what we gave up on". These tests pin
 * the bound down, because an unbounded tombstone store would be a leak and a window that is too short
 * would turn a legal late answer back into a protocol violation.
 */
const limits: RegistryLimits = {
  maxActive: 8,
  retiredCapacity: 4,
  retiredTtlMs: 1_000,
  unresolvedCapacity: 8,
};

function begin(registry: RequestRegistry<string, { tag: string }>, id: string, timeout = 5_000) {
  return registry.begin(id, {
    commandInvocationId: `civ_${id}`,
    payload: { tag: id },
    timeoutMilliseconds: timeout,
  });
}

/** The same registration, measured against the reserved control pools instead of the business ones. */
function beginControl(
  registry: RequestRegistry<string, { tag: string }>,
  id: string,
  timeout = 5_000,
) {
  return registry.begin(id, {
    commandInvocationId: `civ_${id}`,
    payload: { tag: id },
    requestClass: 'control',
    timeoutMilliseconds: timeout,
  });
}

/** A request that really was handed to the writer before the client gave up on it. */
function beginSubmitted(registry: RequestRegistry<string, { tag: string }>, id: string): void {
  begin(registry, id);
  registry.markSubmitted(id);
}

/** Assert the local refusal AND which pool refused, because "which capacity" is part of the answer. */
function expectRefusal(run: () => unknown, refusal: AdmissionRefusal): void {
  try {
    run();
  } catch (error) {
    expect(error).toBeInstanceOf(RequestAdmissionError);
    expect((error as RequestAdmissionError).refusal).toBe(refusal);
    return;
  }
  throw new Error(`admission should have been refused with ${refusal}`);
}

describe('RequestRegistry', () => {
  it('settles exactly once under a timeout/response race', () => {
    const registry = new RequestRegistry<string, { tag: string }>(limits, () => 0);
    begin(registry, 'a');

    const first = registry.settle('a', 'response');
    expect(first?.payload.tag).toBe('a');
    // The losing side of the race gets nothing, so it cannot settle the caller a second time.
    expect(registry.settle('a', 'timeout')).toBeUndefined();
    expect(registry.activeCount).toBe(0);
  });

  it('keeps the caller payload out of the retired record', () => {
    const registry = new RequestRegistry<string, { tag: string }>(limits, () => 0);
    beginSubmitted(registry, 'a');
    registry.settle('a', 'timeout');
    // Only identity is retained: nothing that could hold a response value or a secret survives.
    expect(registry.unresolvedCount).toBe(1);
    expect(registry.classifyLate('a')).toBe('legal-late');
  });

  it('retains identity only, never a payload or a secret', () => {
    const secret = 'synthetic-secret-that-must-not-survive';
    const registry = new RequestRegistry<string, { tag: string }>(limits, () => 0);
    begin(registry, 'a');
    registry.begin('with-secret', {
      commandInvocationId: 'civ_with-secret',
      payload: { tag: secret },
      timeoutMilliseconds: 1_000,
    });
    registry.markSubmitted('with-secret');
    registry.settle('with-secret', 'timeout');

    for (const record of registry.unresolvedRecords()) {
      // The retained record is a closed set of identity/disposition fields, so a future field that
      // carried a payload or a secret would fail this assertion rather than leak quietly. `method` is a
      // NAME retained so a legal late terminal can be reported for THIS request id; it carries no params.
      expect(Object.keys(record).sort()).toEqual([
        'commandInvocationId',
        'finalResponseSeen',
        'id',
        'method',
        'provablyUnsent',
        'retiredAt',
        'trigger',
      ]);
      expect(JSON.stringify(record)).not.toContain(secret);
    }
    expect(registry.unresolvedRecords()).toHaveLength(1);

    // Consuming the real terminal moves the record to the completed history - still identity only.
    expect(registry.classifyLate('with-secret')).toBe('legal-late');
    expect(JSON.stringify(registry.auditRecords())).not.toContain(secret);
    expect(JSON.stringify(registry.stats())).not.toContain(secret);
  });

  it('distinguishes a legal late answer from a duplicate terminal', () => {
    const registry = new RequestRegistry<string, { tag: string }>(limits, () => 0);
    beginSubmitted(registry, 'client-gave-up');
    registry.settle('client-gave-up', 'timeout');
    begin(registry, 'answered');
    registry.settle('answered', 'response');

    expect(registry.classifyLate('client-gave-up')).toBe('legal-late');
    expect(registry.classifyLate('answered')).toBe('duplicate');
    expect(registry.classifyLate('never-mine')).toBe('unknown');
  });

  it('consumes the first legal late terminal exactly once and rejects the duplicate after it', () => {
    // Registry counterexample `duplicate-after-legal-late`: submitted -> timeout -> classifyLate twice
    // used to answer `legal-late` both times, so a second final response was treated as legitimate.
    const registry = new RequestRegistry<string, { tag: string }>(limits, () => 0);
    beginSubmitted(registry, 'sent');
    registry.settle('sent', 'timeout');

    expect(registry.classifyLate('sent')).toBe('legal-late');
    expect(registry.classifyLate('sent')).toBe('duplicate');
    expect(registry.stats().late).toBe(1);
    expect(registry.stats().duplicates).toBe(1);
  });

  it('hands back the identity of the record one legal late terminal was consumed for', () => {
    // The per-request source: a caller that has to attribute the consumption to ONE request gets the
    // id, the invocation and the method of the record whose identity was just spent - and nothing else.
    const registry = new RequestRegistry<string, { tag: string }>(limits, () => 0);
    registry.begin('mine', {
      commandInvocationId: 'civ_mine',
      method: 'fs.write_file',
      payload: { tag: 'mine' },
      timeoutMilliseconds: 5_000,
    });
    registry.markSubmitted('mine');
    registry.settle('mine', 'timeout');

    const consumed = registry.consumeLateTerminal('mine');
    expect(consumed.verdict).toBe('legal-late');
    expect(consumed.consumed).toMatchObject({
      commandInvocationId: 'civ_mine',
      finalResponseSeen: true,
      id: 'mine',
      method: 'fs.write_file',
      trigger: 'timeout',
    });
    // Exactly one consumption: the next terminal for the same id consumed nothing to hand back.
    const duplicate = registry.consumeLateTerminal('mine');
    expect(duplicate.verdict).toBe('duplicate');
    expect(duplicate.consumed).toBeUndefined();
    // The verdict-only wrapper is the same rule, never a second implementation of it.
    expect(registry.classifyLate('mine')).toBe('duplicate');
  });

  it('refuses a terminal for a request that never reached the writer', () => {
    // Registry counterexample `terminal-for-unsent`: pending-write -> timeout is withdrawable, so a
    // response to it has no legitimate source and must not be accepted as a legal late answer.
    const registry = new RequestRegistry<string, { tag: string }>(limits, () => 0);
    begin(registry, 'withdrawn');
    registry.settle('withdrawn', 'timeout');

    expect(registry.classifyLate('withdrawn')).toBe('unsent');
    expect(registry.stats().late).toBe(0);
    expect(registry.stats().unsentTerminals).toBe(1);
  });

  it('marks a request unsent only before the frame reached the writer', () => {
    const registry = new RequestRegistry<string, { tag: string }>(limits, () => 0);
    begin(registry, 'never-written');
    begin(registry, 'written');
    registry.markSubmitted('written');

    const unsent = registry.settle('never-written', 'timeout');
    const sent = registry.settle('written', 'timeout');
    expect(unsent).toBeDefined();
    expect(sent).toBeDefined();
    // The distinction is what lets a timeout be reported as `not_sent` honestly, and it is visible in
    // the retirement verdict as well as in the record.
    expect(isProvablyUnsent('pending-write', 'timeout')).toBe(true);
    expect(isProvablyUnsent('submitted', 'timeout')).toBe(false);
    expect(isProvablyUnsent('pending-write', 'response')).toBe(false);
    expect(registry.auditRecords().map((record) => record.provablyUnsent)).toEqual([true]);
    expect(registry.unresolvedRecords().map((record) => record.id)).toEqual(['written']);
    // A request that had begun writing keeps the Unknown outcome: it is never called `not_sent`.
    expect(registry.classifyLate('written')).toBe('legal-late');
    expect(registry.classifyLate('never-written')).toBe('unsent');
  });

  it('keeps a submitted give-up out of the completed history until its terminal arrives', () => {
    const registry = new RequestRegistry<string, { tag: string }>(limits, () => 0);
    beginSubmitted(registry, 'awaiting');
    registry.settle('awaiting', 'timeout');
    expect(registry.unresolvedRecords().map((record) => record.id)).toEqual(['awaiting']);
    expect(registry.auditRecords()).toEqual([]);

    // The one real terminal is consumed, and only then does the id become completed history.
    expect(registry.classifyLate('awaiting')).toBe('legal-late');
    expect(registry.unresolvedRecords()).toEqual([]);
    expect(registry.auditRecords().map((record) => record.finalResponseSeen)).toEqual([true]);
    expect(registry.classifyLate('awaiting')).toBe('duplicate');
  });

  it('evicts completed history oldest-first but never an id still awaiting its terminal', () => {
    const registry = new RequestRegistry<string, { tag: string }>(
      { maxActive: 8, retiredCapacity: 2, retiredTtlMs: RETIRED_TTL_MS, unresolvedCapacity: 8 },
      () => 0,
    );
    beginSubmitted(registry, 'awaiting');
    registry.settle('awaiting', 'timeout');
    // A burst of fast normal calls is exactly what used to push a timed-out id out of the store.
    for (const id of ['a', 'b', 'c']) {
      begin(registry, id);
      registry.settle(id, 'response');
    }

    expect(registry.retiredCount).toBe(2);
    expect(registry.stats().evicted).toBe(1);
    // The evicted id is unknown again; that is the documented edge of the completed history, and for
    // a finished request it is the same protocol violation as `duplicate`.
    expect(registry.classifyLate('a')).toBe('unknown');
    expect(registry.classifyLate('c')).toBe('duplicate');
    // The id that is still awaiting a real terminal was NOT evicted by that burst: its first real
    // terminal is still the one legal late answer instead of a hostile-looking unknown response.
    expect(registry.classifyLate('awaiting')).toBe('legal-late');
  });

  it('expires completed-history ids by age as well as by capacity', () => {
    let now = 0;
    const registry = new RequestRegistry<string, { tag: string }>(limits, () => now);
    begin(registry, 'a');
    registry.settle('a', 'response');
    expect(registry.classifyLate('a')).toBe('duplicate');

    now += limits.retiredTtlMs + 1;
    expect(registry.classifyLate('a')).toBe('unknown');
    expect(registry.retiredCount).toBe(0);
  });

  it('refuses new work when the channel can no longer account for it', () => {
    // Bounded admission. Both refusals are local: nothing was written, and no retained identity is
    // dropped to make room - that is what keeps a first late answer from becoming `unknown`.
    const active = new RequestRegistry<string, { tag: string }>(
      { maxActive: 1, retiredCapacity: 4, retiredTtlMs: 1_000, unresolvedCapacity: 4 },
      () => 0,
    );
    begin(active, 'a');
    expect(() => begin(active, 'b')).toThrow(RequestAdmissionError);
    try {
      begin(active, 'b');
    } catch (error) {
      expect((error as RequestAdmissionError).refusal).toBe('active-capacity');
    }

    const unresolved = new RequestRegistry<string, { tag: string }>(
      { maxActive: 4, retiredCapacity: 4, retiredTtlMs: 1_000, unresolvedCapacity: 1 },
      () => 0,
    );
    beginSubmitted(unresolved, 'awaiting');
    unresolved.settle('awaiting', 'timeout');
    try {
      begin(unresolved, 'b');
      throw new Error('admission should have been refused');
    } catch (error) {
      expect(error).toBeInstanceOf(RequestAdmissionError);
      expect((error as RequestAdmissionError).refusal).toBe('unresolved-capacity');
    }
    expect(unresolved.stats().admissionRefusals).toBe(1);
    // The refused request never entered the channel, and the outstanding identity survives.
    expect(unresolved.activeCount).toBe(0);
    expect(unresolved.classifyLate('awaiting')).toBe('legal-late');
  });

  it('refuses to reuse an id that is still known to the channel', () => {
    const registry = new RequestRegistry<string, { tag: string }>(limits, () => 0);
    beginSubmitted(registry, 'a');
    expect(() => begin(registry, 'a')).toThrow(/reused/);
    registry.settle('a', 'timeout');
    // Still known while its terminal is outstanding: reuse would make a late answer ambiguous.
    expect(() => begin(registry, 'a')).toThrow(/reused/);
    registry.classifyLate('a');
    expect(() => begin(registry, 'a')).toThrow(/reused/);
  });

  it('expires completed history by elapsed time rather than a wall-clock adjustment', () => {
    const clock = vi.spyOn(performance, 'now').mockReturnValue(20);
    const wallClock = vi.spyOn(Date, 'now').mockReturnValue(100);
    try {
      const registry = new RequestRegistry<string, { tag: string }>(limits);
      begin(registry, 'answered');
      registry.settle('answered', 'response');
      beginSubmitted(registry, 'awaiting');
      registry.settle('awaiting', 'timeout');

      // A wall-clock jump must not forget anything: identity that is still owed a real terminal stays
      // recognisable, and neither must completed history expire early.
      wallClock.mockReturnValue(100_000);
      expect(registry.classifyLate('awaiting')).toBe('legal-late');
      expect(registry.classifyLate('answered')).toBe('duplicate');

      clock.mockReturnValue(20 + limits.retiredTtlMs + 1);
      expect(registry.classifyLate('answered')).toBe('unknown');
    } finally {
      clock.mockRestore();
      wallClock.mockRestore();
    }
  });

  it('drains the active set once so a disconnect cannot settle anyone twice', () => {
    const registry = new RequestRegistry<string, { tag: string }>(limits, () => 0);
    begin(registry, 'a');
    begin(registry, 'b');
    expect(registry.drainActive().map((record) => record.id)).toEqual(['a', 'b']);
    expect(registry.drainActive()).toEqual([]);
    expect(registry.activeCount).toBe(0);
  });

  it('measures control admission against its own pools, not the saturated business ones', () => {
    // Control counterexample: a full business set used to refuse the very entry point that resolves it.
    const registry = new RequestRegistry<string, { tag: string }>(
      {
        controlCapacity: 2,
        controlUnresolvedCapacity: 2,
        maxActive: 2,
        retiredCapacity: 4,
        retiredTtlMs: 1_000,
      },
      () => 0,
    );
    // Business capacity saturated in BOTH pools: two identities awaiting terminals, two more in flight.
    beginSubmitted(registry, 'b1');
    beginSubmitted(registry, 'b2');
    registry.settle('b1', 'timeout');
    registry.settle('b2', 'timeout');
    begin(registry, 'b3');
    begin(registry, 'b4');
    expect(registry.unresolvedCount).toBe(2);
    expectRefusal(() => begin(registry, 'b5'), 'active-capacity');
    expectRefusal(() => begin(registry, 'b6'), 'active-capacity');

    // Control work is admitted from its own pools while every business pool is full.
    const control = beginControl(registry, 'c1');
    expect(control.requestClass).toBe('control');
    expect(registry.activeCount).toBe(3);
    expect(registry.activeControlCount).toBe(1);
    expect(registry.unresolvedControlCount).toBe(0);
    // Neither refusal was a control refusal: the reserved pool was never what said no.
    expect(registry.stats()).toMatchObject({ admissionRefusals: 2, controlAdmissionRefusals: 0 });
  });

  it('treats an undeclared class as ordinary, so a caller cannot drift into reserved capacity', () => {
    const registry = new RequestRegistry<string, { tag: string }>(
      { controlCapacity: 1, controlUnresolvedCapacity: 1, retiredCapacity: 4, retiredTtlMs: 1_000 },
      () => 0,
    );
    const plain = begin(registry, 'plain');
    expect(plain.requestClass).toBe('ordinary');
    expect(registry.activeControlCount).toBe(0);
    registry.markSubmitted('plain');
    registry.settle('plain', 'timeout');
    // The identity of a request that never declared itself control stays in the business pool.
    expect(registry.unresolvedCount).toBe(1);
    expect(registry.unresolvedControlCount).toBe(0);
    expect(registry.classifyLate('plain')).toBe('legal-late');
  });

  it('refuses control work from its own pools and never evicts an identity still owed a terminal', () => {
    const registry = new RequestRegistry<string, { tag: string }>(
      {
        controlCapacity: 1,
        controlUnresolvedCapacity: 1,
        maxActive: 4,
        retiredCapacity: 4,
        retiredTtlMs: 1_000,
      },
      () => 0,
    );
    beginControl(registry, 'c1');
    registry.markSubmitted('c1');
    // In-flight control capacity is full: refused locally, and business work is untouched by it.
    expectRefusal(() => beginControl(registry, 'c2'), 'control-capacity');
    begin(registry, 'b1');

    registry.settle('c1', 'timeout');
    expect(registry.unresolvedCount).toBe(0);
    expect(registry.unresolvedControlCount).toBe(1);
    expect(registry.controlUnresolvedRecords().map((record) => record.id)).toEqual(['c1']);
    expectRefusal(() => beginControl(registry, 'c3'), 'control-unresolved-capacity');
    expect(registry.unresolvedControlCount).toBe(1);
    expect(registry.stats()).toMatchObject({ admissionRefusals: 2, controlAdmissionRefusals: 2 });

    // The identity survives the refusals: its first real terminal is still the one legal late answer.
    expect(registry.classifyLate('c1')).toBe('legal-late');
    expect(registry.classifyLate('c1')).toBe('duplicate');
    expect(registry.activeCount).toBe(1);
    expect(registry.activeControlCount).toBe(0);
  });

  it('bounds unresolved identity by the threshold PLUS the requests already admitted', () => {
    // The real bound, computed from the states: a threshold is what admission is measured AGAINST, and
    // requests admitted while the pool was still below it can retire into it afterwards. With small
    // numbers, `unresolvedCapacity: 2` and `maxActive: 4` reach 5 = threshold - 1 + maxActive; the
    // conservative design bound is threshold + maxActive (768 for the production business pools).
    const registry = new RequestRegistry<string, { tag: string }>(
      { maxActive: 4, retiredCapacity: 4, retiredTtlMs: 1_000, unresolvedCapacity: 2 },
      () => 0,
    );
    beginSubmitted(registry, 'first');
    registry.settle('first', 'timeout');
    for (const id of ['a', 'b', 'c', 'd']) beginSubmitted(registry, id);
    for (const id of ['a', 'b', 'c', 'd']) registry.settle(id, 'timeout');

    expect(registry.unresolvedCount).toBe(5);
    expect(registry.unresolvedCount).toBeLessThanOrEqual(2 - 1 + 4);
    expectRefusal(() => begin(registry, 'later'), 'unresolved-capacity');
    // Nothing was evicted to make room, and every identity still recognises its first real terminal.
    expect(registry.unresolvedRecords()).toHaveLength(5);
    expect(registry.classifyLate('a')).toBe('legal-late');
    expect(registry.classifyLate('first')).toBe('legal-late');
  });

  it('keeps ONE identity space, so neither class can reuse an id the other still owes', () => {
    const registry = new RequestRegistry<string, { tag: string }>(limits, () => 0);
    beginSubmitted(registry, 'business');
    registry.settle('business', 'timeout');
    expect(() => beginControl(registry, 'business')).toThrow(/reused/);
    beginControl(registry, 'control');
    expect(() => begin(registry, 'control')).toThrow(/reused/);
    // Both pools answer to the same duplicate/legal-late rules once their terminal arrives.
    expect(registry.classifyLate('business')).toBe('legal-late');
    expect(registry.classifyLate('business')).toBe('duplicate');
  });

  it('ships bounded defaults', () => {
    expect(RETIRED_CAPACITY).toBeGreaterThan(0);
    // Must comfortably exceed the longest per-request budget used on this channel (180s build
    // extension), or a genuine late answer would fall outside the window.
    expect(RETIRED_TTL_MS).toBeGreaterThan(180_000);
    expect(MAX_ACTIVE_REQUESTS).toBeGreaterThan(0);
    expect(UNRESOLVED_TERMINAL_CAPACITY).toBeGreaterThan(0);
    expect(MAX_CONTROL_REQUESTS).toBeGreaterThan(0);
    expect(CONTROL_UNRESOLVED_TERMINAL_CAPACITY).toBeGreaterThan(0);
    // The reserved pools stay far below the business pools: this is a reserved ENTRY POINT, not a
    // second business capacity, so control traffic cannot outgrow the work it exists to recover.
    expect(MAX_CONTROL_REQUESTS).toBeLessThan(MAX_ACTIVE_REQUESTS);
    expect(CONTROL_UNRESOLVED_TERMINAL_CAPACITY).toBeLessThan(UNRESOLVED_TERMINAL_CAPACITY);
    expect(() => new RequestRegistry(limits, () => 0)).not.toThrow();
  });
});

/**
 * Request-lifetime ownership for one authenticated Main<->Core channel.
 *
 * The problem this replaces: a single request timing out used to retire the WHOLE channel, and any
 * inbound id that was not in the pending map was treated as a protocol violation. Both made one slow
 * request the business of every other request on the connection.
 *
 * The rule this module implements instead:
 *
 *  - `active` holds waiters that have not settled yet. It is bounded by admission per class
 *    (`maxActive` for business work, `MAX_CONTROL_REQUESTS` for the reserved control routes), so a
 *    burst of new work is refused locally (overload) instead of growing the pending set. A timeout
 *    settles exactly one entry.
 *  - Identity after settlement is split in two, because the two sets carry different obligations:
 *
 *    1. `unresolved` - requests that WERE submitted and then settled by the client without a
 *       response. Their first inbound terminal is a legal late answer. Evicting one by capacity would
 *       disguise a real answer as a hostile unknown response, so this set is never evicted by
 *       capacity or age: admission is refused once it is full, and an entry leaves only when its
 *       terminal is consumed (it then moves to the audit history) or with the channel itself. There
 *       is one such pool per admission class (see below), with the same rules in each.
 *    2. `audit` - the completed history: ids whose final response was already seen (delivered to the
 *       waiter, or consumed as the single legal late terminal), plus ids that were provably never
 *       sent. It is bounded by capacity AND age, oldest-first. For these ids a forgotten record can
 *       only turn `duplicate`/`unsent` into `unknown`, and all three are protocol violations, so
 *       eviction here cannot hide a legitimate answer.
 *
 *  - A late terminal is only "legal" when the client itself gave up on a request it had actually
 *    submitted. A SECOND terminal for an id whose final response was already seen is a duplicate, a
 *    terminal for an id that never reached the writer is `unsent`, and an id that was never ours (or
 *    that aged out of the completed history) is unknown. Only the first legal late terminal is
 *    consumed, exactly once, and it is never fed back to the settled caller. These rules are the same
 *    for every admission class.
 *  - Secrets are never copied here. Only identity is retained.
 *
 * Admission is split into two classes, each with its OWN active bound and its OWN identity bound:
 *
 *  - `ordinary` - business work. Its two pools are the ones every product request uses.
 *  - `control` - the internal cancel / outcome-query routes. Keeping unresolved identity is correct,
 *    but it must not make the entry point that resolves it unreachable: a channel whose `ordinary`
 *    pools are saturated still admits control work, and control work keeps its own identity
 *    obligations instead of borrowing business capacity.
 *
 * WHICH CLASS A REQUEST GETS IS NOT THE CALLER'S CHOICE. It is decided by the transport's internal
 * route table, which is the only place that passes `control`; a caller-supplied method string, a
 * caller-supplied priority or a caller-supplied marker all reach `begin` as `ordinary` (the default),
 * so no model-reachable input can spend reserved control capacity.
 *
 * Capacity, retention and exit policy for ONE channel, with the production numbers:
 *
 *  - `ordinary active`: at most MAX_ACTIVE_REQUESTS (512) unsettled requests. Exit: settlement
 *    (response, timeout, cancel, send failure) or drain on channel close. Over capacity the channel
 *    refuses new work locally with `RequestAdmissionError` instead of growing the set.
 *  - `ordinary unresolved`: at most UNRESOLVED_TERMINAL_CAPACITY (256) ids at admission. Retention:
 *    the life of the channel. Exit: the first real terminal (the record moves to the audit history) or
 *    the end of the channel itself. It is NEVER evicted, so "awaiting its terminal" cannot be lost by
 *    capacity. Over capacity admission is refused; a settlement already in flight still keeps its
 *    identity, so the pool can exceed the threshold by whatever was already admitted and stays bounded.
 *  - `control active` / `control unresolved`: the same rules and the same identity obligations, in
 *    their own pools, bounded by MAX_CONTROL_REQUESTS (16) and CONTROL_UNRESOLVED_TERMINAL_CAPACITY
 *    (64). A full control pool is refused locally exactly like a full business pool, and no identity
 *    awaiting its real terminal is evicted to make room.
 *  - `audit`: at most RETIRED_CAPACITY (256) ids and RETIRED_TTL_MS (300 000 ms) of age, measured on
 *    the monotonic clock; shared by both classes. Exit: oldest-first eviction or age. Eviction here can
 *    only turn a `duplicate` or `unsent` verdict into `unknown`, and all three are protocol violations,
 *    so it can never disguise a legitimate first late answer as a hostile unknown response.
 *
 * THE REAL UPPER BOUND, derived from those states rather than from the admission thresholds. A
 * threshold is what admission is measured against, NOT the size a pool can reach: requests admitted
 * while the pool was still below its threshold can retire into it afterwards.
 *
 *  - ordinary identity: admission requires `unresolved < 256` and `active <= 512`, and each settlement
 *    turns at most one admitted request into one unresolved record. So the conservative bound is
 *    UNRESOLVED_TERMINAL_CAPACITY + MAX_ACTIVE_REQUESTS = 768 ids (the reachable maximum is 767,
 *    because the last successful admission happens while the pool is still one below its threshold).
 *    "Unresolved is always <= 256" is wrong: 256 is only the admission threshold.
 *  - control identity: CONTROL_UNRESOLVED_TERMINAL_CAPACITY + MAX_CONTROL_REQUESTS = 80 ids, by the
 *    same arithmetic.
 *  - completed history: RETIRED_CAPACITY = 256 ids.
 *
 * So ONE channel's identity store holds at most 768 + 80 + 256 = 1104 identity records, and at most
 * MAX_ACTIVE_REQUESTS + MAX_CONTROL_REQUESTS = 528 unsettled requests exist at any moment.
 */

/**
 * Where a request is in its lifetime, as the transport reports it as a lifecycle record.
 *
 * `pending-write` and `submitted` are the two SEND stages a live record can hold: `markSubmitted`
 * moves a record from the first to the second and nothing else ever changes the stored stage.
 *
 * `late-terminal-consumed` is never stored on a live record. It is the stage of the identity-only
 * lifecycle record the transport emits at the moment it consumes the first real terminal of a request
 * its caller had already given up on. It lives in this union - rather than being a counter a reader has
 * to attribute to some request - because "this request's real terminal arrived and was consumed" is a
 * fact about ONE request, and the channel-wide `late` count cannot express it.
 */
export type RequestStage = 'pending-write' | 'submitted' | 'late-terminal-consumed';

/**
 * Which admission pool a request belongs to.
 *
 * `control` is reserved for this file's internal callers and is passed by the transport's own route
 * table. It is deliberately NOT derivable from a method string, a priority or any other request data,
 * so a model-reachable caller cannot obtain reserved capacity by naming itself.
 */
export type RequestClass = 'ordinary' | 'control';

export type SettlementTrigger = 'response' | 'timeout' | 'cancel' | 'send-failure' | 'disconnect';

/**
 * `legal-late` - the first real terminal after the client gave up; consumed exactly once.
 * `duplicate`  - the final response for this id was already seen; a second one is a defect.
 * `unsent`     - the client never handed this id to the writer; no terminal can legitimately exist.
 * `unknown`    - never ours, or aged out of the completed history.
 */
export type LateTerminalVerdict = 'legal-late' | 'duplicate' | 'unsent' | 'unknown';

/**
 * The verdict for one inbound terminal, together with the record it was decided on.
 *
 * `consumed` is present exactly when `verdict === 'legal-late'`: it is the retired record whose
 * identity was just spent, and it is what lets a caller report the consumption of ONE request's real
 * terminal instead of reading a channel-wide count. Every other verdict consumed nothing.
 */
export interface LateTerminalConsumption<Id extends string> {
  readonly consumed: RetiredRecord<Id> | undefined;
  readonly verdict: LateTerminalVerdict;
}

/**
 * Why a request was refused before anything was written. All four are local overload, not a Core
 * fault: nothing was written, so there is nothing to query and nothing to replay.
 */
export type AdmissionRefusal =
  'active-capacity' | 'unresolved-capacity' | 'control-capacity' | 'control-unresolved-capacity';

/**
 * Bounded admission refused this request. The caller must treat it as "definitely not sent": nothing
 * was written, so there is nothing to query and nothing to replay.
 */
export class RequestAdmissionError extends Error {
  readonly refusal: AdmissionRefusal;

  constructor(refusal: AdmissionRefusal, message: string) {
    super(message);
    this.name = 'RequestAdmissionError';
    this.refusal = refusal;
  }
}

export interface RetiredRecord<Id extends string> {
  readonly id: Id;
  readonly commandInvocationId: string | undefined;
  /**
   * The invocation NAME this request was really sent as, kept for the same reason the invocation id is:
   * the one legal late terminal for this id is reported as a lifecycle record carrying the request id,
   * the invocation id, the method and the channel, so the request's identity has to outlive its waiter
   * for that to be possible. It is a method name, never params, a body or a secret.
   */
  readonly method: string | undefined;
  readonly retiredAt: number;
  readonly trigger: SettlementTrigger;
  /** True when the settlement happened before the frame reached the writer. */
  readonly provablyUnsent: boolean;
  /**
   * True once the Core's final response for this id has been seen: either delivered to the waiter
   * (`trigger === 'response'`), or consumed as the one legal late terminal.
   */
  readonly finalResponseSeen: boolean;
}

export interface ActiveRecord<Id extends string, Payload = undefined> {
  readonly id: Id;
  readonly commandInvocationId: string | undefined;
  readonly deadline: number;
  /**
   * The invocation name this request is being sent as. Retained so a legal late terminal can be
   * reported as a record about THIS request after the waiter is gone; see `RetiredRecord.method`.
   */
  readonly method: string | undefined;
  /**
   * Opaque caller-owned value (the settle callbacks). It lives HERE rather than in a second map so the
   * channel has exactly one owner for "which requests exist"; two maps would be two sources of truth.
   * It is deliberately absent from the retired record, so nothing survives settlement but identity.
   */
  readonly payload: Payload;
  /**
   * The admission class this request was admitted into. It decides which unresolved pool the record
   * retires into; nothing else about settlement depends on it.
   */
  readonly requestClass: RequestClass;
  stage: RequestStage;
}

export interface RegistryLimits {
  /**
   * Maximum ORDINARY unsettled requests admitted at once. Beyond this the channel refuses new work
   * locally (overload) instead of dispatching into a set it can no longer account for.
   * Omitted: `MAX_ACTIVE_REQUESTS`.
   */
  readonly maxActive?: number;
  /**
   * Maximum ids of submitted ORDINARY requests still awaiting their real terminal. This set is never
   * evicted: once it is full, admission is refused and the entries leave by their real terminal.
   * Omitted: `UNRESOLVED_TERMINAL_CAPACITY`.
   */
  readonly unresolvedCapacity?: number;
  /**
   * Maximum CONTROL (cancel / outcome-query) unsettled requests admitted at once. A separate pool from
   * `maxActive` on purpose: a business backlog must not be able to refuse the entry point that
   * resolves it. Omitted: `MAX_CONTROL_REQUESTS`.
   */
  readonly controlCapacity?: number;
  /**
   * Maximum ids of submitted CONTROL requests still awaiting their real terminal. Never evicted, same
   * rules as `unresolvedCapacity`. Omitted: `CONTROL_UNRESOLVED_TERMINAL_CAPACITY`.
   */
  readonly controlUnresolvedCapacity?: number;
  /** Maximum retained ids in the completed history. Eviction is oldest-first. */
  readonly retiredCapacity: number;
  /**
   * How long a completed-history id stays recognisable. This MUST exceed the longest per-request
   * budget on the channel, or a genuine duplicate/unsent terminal falls outside the window and is
   * reported as unknown again.
   */
  readonly retiredTtlMs: number;
}

/** Bounded admission for unsettled ORDINARY requests on one channel. */
export const MAX_ACTIVE_REQUESTS = 512;
/**
 * Identity of submitted ORDINARY requests still awaiting their real terminal. Admission is refused at
 * this size rather than evicting an entry, because an evicted entry would turn a legitimate first late
 * answer into an unknown-response protocol violation. This is an admission THRESHOLD: requests already
 * admitted can still retire into the pool, so the pool itself is bounded by this plus
 * `MAX_ACTIVE_REQUESTS` (see the module comment).
 */
export const UNRESOLVED_TERMINAL_CAPACITY = 256;
/**
 * Bounded admission for unsettled CONTROL requests (cancel, command-outcome query) on one channel.
 *
 * WHY IT IS SMALL: control calls are issued one per operator or automation action, and the supervisor
 * awaits each one, so 16 in flight is already an order of magnitude above the product's real
 * concurrency. Reserving MORE than that would let control traffic outgrow the work it exists to
 * recover.
 */
export const MAX_CONTROL_REQUESTS = 16;
/**
 * Identity of submitted CONTROL requests still awaiting their real terminal.
 *
 * WHY IT IS THIS SIZE: a control identity is only created when a control call was really sent and then
 * never answered. 64 unanswered control calls on one channel already means the channel is not carrying
 * control traffic at all; past that, further control admission is refused locally (honestly, with
 * nothing written) rather than growing the identity store without limit. Existing identities are kept,
 * so the refused recovery can be retried once any real terminal arrives.
 */
export const CONTROL_UNRESOLVED_TERMINAL_CAPACITY = 64;
export const RETIRED_CAPACITY = 256;
export const RETIRED_TTL_MS = 300_000;

export class RequestRegistry<Id extends string = string, Payload = undefined> {
  readonly #active = new Map<Id, ActiveRecord<Id, Payload>>();
  /** ORDINARY submitted, client gave up, terminal not seen yet. Never evicted; admission refused. */
  readonly #unresolved = new Map<Id, RetiredRecord<Id>>();
  /** CONTROL submitted, client gave up, terminal not seen yet. Same rules, own bound. */
  readonly #controlUnresolved = new Map<Id, RetiredRecord<Id>>();
  /** Completed history of BOTH classes. Insertion-ordered, which makes oldest-first eviction `keys().next()`. */
  readonly #audit = new Map<Id, RetiredRecord<Id>>();
  readonly #limits: RegistryLimits & {
    controlCapacity: number;
    controlUnresolvedCapacity: number;
    maxActive: number;
    unresolvedCapacity: number;
  };
  readonly #now: () => number;
  #admissionRefusals = 0;
  #controlAdmissionRefusals = 0;
  #evicted = 0;
  #late = 0;
  #duplicates = 0;
  #unsent = 0;

  constructor(limits: RegistryLimits, now: () => number = () => performance.now()) {
    if (!Number.isSafeInteger(limits.retiredCapacity) || limits.retiredCapacity < 1) {
      throw new Error('retiredCapacity must be a positive safe integer');
    }
    if (!Number.isSafeInteger(limits.retiredTtlMs) || limits.retiredTtlMs < 1) {
      throw new Error('retiredTtlMs must be a positive safe integer');
    }
    const maxActive = limits.maxActive ?? MAX_ACTIVE_REQUESTS;
    const unresolvedCapacity = limits.unresolvedCapacity ?? UNRESOLVED_TERMINAL_CAPACITY;
    const controlCapacity = limits.controlCapacity ?? MAX_CONTROL_REQUESTS;
    const controlUnresolvedCapacity =
      limits.controlUnresolvedCapacity ?? CONTROL_UNRESOLVED_TERMINAL_CAPACITY;
    if (!Number.isSafeInteger(maxActive) || maxActive < 1) {
      throw new Error('maxActive must be a positive safe integer');
    }
    if (!Number.isSafeInteger(unresolvedCapacity) || unresolvedCapacity < 1) {
      throw new Error('unresolvedCapacity must be a positive safe integer');
    }
    if (!Number.isSafeInteger(controlCapacity) || controlCapacity < 1) {
      throw new Error('controlCapacity must be a positive safe integer');
    }
    if (!Number.isSafeInteger(controlUnresolvedCapacity) || controlUnresolvedCapacity < 1) {
      throw new Error('controlUnresolvedCapacity must be a positive safe integer');
    }
    this.#limits = {
      ...limits,
      controlCapacity,
      controlUnresolvedCapacity,
      maxActive,
      unresolvedCapacity,
    };
    this.#now = now;
  }

  get activeCount(): number {
    return this.#active.size;
  }

  /** How many of the unsettled requests are control work. The rest are ordinary business work. */
  get activeControlCount(): number {
    return this.#activeCountOf('control');
  }

  get retiredCount(): number {
    return this.#audit.size;
  }

  get unresolvedCount(): number {
    return this.#unresolved.size;
  }

  get unresolvedControlCount(): number {
    return this.#controlUnresolved.size;
  }

  /**
   * Whether this request is still unsettled.
   *
   * The write path consults this so a request the caller already gave up on is WITHDRAWN from the
   * outbound queue rather than dispatched silently after its waiter was rejected. Without this check
   * `not_sent` would describe only the caller's state while the bytes still went out - exactly the
   * "reject the waiter, then let the write dispatch with no notice" failure.
   */
  isActive(id: Id): boolean {
    return this.#active.has(id);
  }

  stats(): {
    /** Every unsettled request on the channel; `activeControl` is the control part of it. */
    active: number;
    activeControl: number;
    admissionRefusals: number;
    audit: number;
    /** Refusals that were charged to the control class (part of `admissionRefusals`). */
    controlAdmissionRefusals: number;
    duplicates: number;
    evicted: number;
    late: number;
    /** ORDINARY identities awaiting their real terminal (bounded by `unresolvedCapacity`). */
    unresolved: number;
    /** CONTROL identities awaiting their real terminal (bounded by `controlUnresolvedCapacity`). */
    unresolvedControl: number;
    unsentTerminals: number;
  } {
    return {
      active: this.#active.size,
      activeControl: this.#activeCountOf('control'),
      admissionRefusals: this.#admissionRefusals,
      audit: this.#audit.size,
      controlAdmissionRefusals: this.#controlAdmissionRefusals,
      duplicates: this.#duplicates,
      evicted: this.#evicted,
      late: this.#late,
      unresolved: this.#unresolved.size,
      unresolvedControl: this.#controlUnresolved.size,
      unsentTerminals: this.#unsent,
    };
  }

  /**
   * Register a waiter. The stage starts at `pending-write`: nothing has reached the writer yet.
   *
   * Admission is bounded per class: a full pool refuses this request with `RequestAdmissionError`
   * instead of pushing an entry the channel cannot remember, and it never evicts a request that is
   * still owed a real terminal. A control request is measured against the control pools only, so a
   * saturated business pool cannot refuse the cancel/outcome-query path that drains it.
   */
  begin(
    id: Id,
    options: {
      commandInvocationId: string | undefined;
      /**
       * The invocation name this request is being sent as. Optional so a caller that does not report
       * one keeps a registry that stores no more than it did; a request without it simply has no method
       * to put on a late-terminal record, and that fact is then left unreported rather than invented.
       */
      method?: string;
      payload: Payload;
      /**
       * Which admission pool to measure this request against. Omitted means `ordinary`, so forgetting
       * it can only cost an internal caller its reserved capacity, never grant it to anyone else.
       */
      requestClass?: RequestClass;
      timeoutMilliseconds: number;
    },
  ): ActiveRecord<Id, Payload> {
    const requestClass = options.requestClass ?? 'ordinary';
    if (
      this.#active.has(id) ||
      this.#unresolved.has(id) ||
      this.#controlUnresolved.has(id) ||
      this.#audit.has(id)
    ) {
      throw new Error('request id was reused while still known to this channel');
    }
    if (requestClass === 'control') {
      const capacity = this.#limits.controlCapacity;
      if (this.#activeCountOf('control') >= capacity) {
        throw this.#refuse(
          'control-capacity',
          `channel already has ${capacity} unsettled control requests`,
          true,
        );
      }
      const identity = this.#limits.controlUnresolvedCapacity;
      if (this.#controlUnresolved.size >= identity) {
        throw this.#refuse(
          'control-unresolved-capacity',
          `channel is still awaiting the real terminal of ${identity} submitted control requests`,
          true,
        );
      }
    } else {
      const capacity = this.#limits.maxActive;
      if (this.#activeCountOf('ordinary') >= capacity) {
        throw this.#refuse('active-capacity', `channel already has ${capacity} unsettled requests`);
      }
      const identity = this.#limits.unresolvedCapacity;
      if (this.#unresolved.size >= identity) {
        throw this.#refuse(
          'unresolved-capacity',
          `channel is still awaiting the real terminal of ${identity} submitted requests`,
        );
      }
    }
    const record: ActiveRecord<Id, Payload> = {
      commandInvocationId: options.commandInvocationId,
      deadline: this.#now() + options.timeoutMilliseconds,
      id,
      method: options.method,
      payload: options.payload,
      requestClass,
      stage: 'pending-write',
    };
    this.#active.set(id, record);
    return record;
  }

  /**
   * Called immediately before the frame is handed to the writer. From this point the client cannot
   * prove the bytes were not written, so the request stops being reportable as `not_sent`.
   */
  markSubmitted(id: Id): void {
    const record = this.#active.get(id);
    if (record !== undefined) record.stage = 'submitted';
  }

  /**
   * Settle one request and hand back its payload. Returns `undefined` when it was already settled,
   * which is what makes every race (timeout vs response, disconnect vs response) settle exactly once.
   */
  settle(id: Id, trigger: SettlementTrigger): ActiveRecord<Id, Payload> | undefined {
    const record = this.#active.get(id);
    if (record === undefined) return undefined;
    this.#active.delete(id);
    this.#retire(
      {
        commandInvocationId: record.commandInvocationId,
        finalResponseSeen: trigger === 'response',
        id,
        method: record.method,
        provablyUnsent: isProvablyUnsent(record.stage, trigger),
        retiredAt: this.#now(),
        trigger,
      },
      record.requestClass,
    );
    return record;
  }

  /**
   * Classify an inbound terminal for an id we do not currently have active, handing back BOTH the
   * verdict and the identity of the record it was decided on.
   *
   * Only the first real terminal after a client give-up is consumed, and it is consumed exactly once:
   * the record moves to the completed history with `finalResponseSeen`, so any further terminal is a
   * duplicate. Both classes share one identity space, so this is the same rule for business and control
   * ids.
   *
   * WHY THE CONSUMED RECORD IS HANDED BACK: `consumed` is present exactly when the verdict is
   * `legal-late`, i.e. exactly when "the designated request's real terminal arrived after its caller
   * gave up" became true. A caller that has to REPORT that for one request - rather than read a
   * channel-wide counter which cannot say whose terminal it was - needs the id, the invocation id and
   * the method of the record whose identity was just spent. Handing it back here keeps that fact in the
   * registry, the single owner of "which requests exist", instead of in a second map beside it.
   */
  consumeLateTerminal(id: Id): LateTerminalConsumption<Id> {
    this.sweep();
    const awaited = this.#unresolved.get(id) ?? this.#controlUnresolved.get(id);
    if (awaited !== undefined) {
      // Exactly one of the two pools owns this identity; removing it from both is the same one removal.
      const fromControl = !this.#unresolved.delete(id);
      this.#controlUnresolved.delete(id);
      this.#late += 1;
      const consumed: RetiredRecord<Id> = {
        ...awaited,
        finalResponseSeen: true,
        retiredAt: this.#now(),
      };
      this.#retire(consumed, fromControl ? 'control' : 'ordinary');
      return { consumed, verdict: 'legal-late' };
    }
    const record = this.#audit.get(id);
    if (record === undefined) return { consumed: undefined, verdict: 'unknown' };
    if (record.finalResponseSeen) {
      this.#duplicates += 1;
      return { consumed: undefined, verdict: 'duplicate' };
    }
    this.#unsent += 1;
    return { consumed: undefined, verdict: 'unsent' };
  }

  /**
   * The verdict alone, for a caller that only has to decide how to answer an inbound terminal.
   *
   * Deliberately a thin wrapper: there is one implementation of the late rule, so the identity that
   * `consumeLateTerminal` reports and the verdict `classifyLate` returns can never disagree.
   */
  classifyLate(id: Id): LateTerminalVerdict {
    return this.consumeLateTerminal(id).verdict;
  }

  /** Drop completed-history ids older than the window, then enforce its capacity. */
  sweep(): void {
    const cutoff = this.#now() - this.#limits.retiredTtlMs;
    for (const [id, record] of this.#audit) {
      if (record.retiredAt > cutoff) break;
      this.#audit.delete(id);
    }
    while (this.#audit.size > this.#limits.retiredCapacity) {
      const oldest = this.#audit.keys().next();
      if (oldest.done === true) break;
      this.#audit.delete(oldest.value);
      this.#evicted += 1;
    }
  }

  /** Every active id, in registration order. Used to reject the channel's in-flight work on close. */
  activeIds(): Id[] {
    return [...this.#active.keys()];
  }

  drainActive(): ActiveRecord<Id, Payload>[] {
    const records = [...this.#active.values()];
    this.#active.clear();
    return records;
  }

  /**
   * Identity-only views. They exist so a test (or a diagnostic) can assert WHAT is retained: these
   * records carry identity and disposition, never a payload, a response value or a secret.
   */
  auditRecords(): readonly RetiredRecord<Id>[] {
    return [...this.#audit.values()];
  }

  /** ORDINARY identities awaiting their real terminal, matching `unresolvedCount`. */
  unresolvedRecords(): readonly RetiredRecord<Id>[] {
    return [...this.#unresolved.values()];
  }

  /** CONTROL identities awaiting their real terminal, matching `unresolvedControlCount`. */
  controlUnresolvedRecords(): readonly RetiredRecord<Id>[] {
    return [...this.#controlUnresolved.values()];
  }

  /**
   * Count unsettled requests of one class from the LIVE set rather than from a parallel counter, so
   * the number admission is measured against is the same state any reader can see and the two can
   * never disagree. The sets are bounded (<= MAX_ACTIVE_REQUESTS + MAX_CONTROL_REQUESTS), and this
   * runs once per admission, so the walk costs less than encoding the frame it admits.
   */
  #activeCountOf(requestClass: RequestClass): number {
    let count = 0;
    for (const record of this.#active.values()) {
      if (record.requestClass === requestClass) count += 1;
    }
    return count;
  }

  #refuse(refusal: AdmissionRefusal, message: string, control = false): RequestAdmissionError {
    this.#admissionRefusals += 1;
    if (control) this.#controlAdmissionRefusals += 1;
    return new RequestAdmissionError(refusal, message);
  }

  #retire(record: RetiredRecord<Id>, requestClass: RequestClass): void {
    if (!record.finalResponseSeen && !record.provablyUnsent) {
      // Submitted, client gave up, real terminal outstanding: this identity is what makes the first
      // real terminal a legal late answer, so it is kept - in the pool of its own class - until that
      // terminal arrives. It is never evicted to make room for anything else.
      const pool = requestClass === 'control' ? this.#controlUnresolved : this.#unresolved;
      pool.set(record.id, record);
      return;
    }
    this.#audit.delete(record.id);
    this.#audit.set(record.id, record);
    this.sweep();
  }
}

/**
 * True when the client can PROVE the frame never reached the writer: the request stopped being
 * awaited while its stage was still `pending-write`. A request that had begun writing, a partial
 * frame, or a committed control frame keeps `Unknown` and must never be reported as `not_sent`.
 */
export const isProvablyUnsent = (stage: RequestStage, trigger: SettlementTrigger): boolean =>
  trigger !== 'response' && stage === 'pending-write';

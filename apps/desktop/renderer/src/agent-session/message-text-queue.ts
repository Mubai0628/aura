/** Progressive message-text loading.
 *
 *  A long body is filled in automatically in bounded pages. Requests run with
 *  bounded concurrency and fair ordering so that many long messages on screen
 *  never stampede the IPC bridge, and no off-screen body is starved.
 *
 *  Two signals decide the order, and the freshest one wins:
 *
 *  - `visible` means "this body is on screen right now", set by the viewport
 *    observer. It outranks background work for one page.
 *  - `auto` means "this body received a page and has more text to read". It is
 *    a continuation and only jumps ahead of work no one has asked for.
 *
 *  A signal is spent by the page it starts. A signal that arrives while a page
 *  is already in flight is kept for the next page instead of being dropped. A
 *  page that failed is never retried behind the user's back: the loader stops
 *  asking until the user presses retry. Only two pages ever run at once. */
interface Task {
  /** Fetch the next page, continuing from `offset` bytes already rendered. */
  run: (signal: AbortSignal, offset: number) => Promise<void>;
  /** The byte the next page must continue from. */
  offset: number;
  /** The callback of the page that is in flight, when it differs from the one
   *  the loader has registered since. */
  next: ((signal: AbortSignal, offset: number) => Promise<void>) | undefined;
  /** False once the owning component unmounted. */
  attached: boolean;
  /** True while a page of this task is in flight. */
  running: boolean;
  /** True while the id sits in the waiting queue. */
  waiting: boolean;
  /** The body is on screen: its next page outranks background work. */
  visible: boolean;
  /** A continuation the loader asked for, not a request from the viewport. */
  auto: boolean;
  /** A request that must jump ahead of other continuations: the viewport
   *  promoted this body, or the user asked for a retry. Spent by the page it
   *  starts. */
  promoted: boolean;
  /** How many promotions this body has received. A page spends the promotion it
   *  started with, and only that one: a promotion that arrived while the page
   *  was in flight is a fresh signal for the next page. */
  promotions: number;
  /** True while the last page of this body failed. A failed page stays out of
   *  the queue until the user asks for a retry. */
  failed: boolean;
  /** Arrival order, for the oldest-request-first rule. */
  order: number;
  /** How many pages had started when this request entered the queue. The wait age
   *  is measured against that, so a continuously re-registered page is always
   *  fresh while a request nobody serves keeps getting older. */
  enqueuedAt: number;
  abort: AbortController | undefined;
}

/** Two concurrent pages: enough to keep the bridge busy without queueing one
 *  IPC request per visible message. */
export const MESSAGE_TEXT_CONCURRENCY = 2;

/** How many pages a queued request may watch go by before it outranks fresh work.
 *  Small enough that background filling keeps moving, large enough that a body the
 *  user is looking at keeps its page-per-page continuity. */
const BURST_PAGES = 8;

export class MessageTextQueue {
  private readonly tasks = new Map<string, Task>();
  private readonly queue: string[] = [];
  private active = 0;
  private order = 0;
  /** Pages started so far. It only ever grows, so `served - enqueuedAt` is how many
   *  pages a queued request has watched other bodies take. */
  private served = 0;

  constructor(private concurrency = MESSAGE_TEXT_CONCURRENCY) {}

  /** Narrow the number of in-flight pages; used by tests that need a strict
   *  order instead of a race. */
  setConcurrency(concurrency: number): void {
    this.concurrency = concurrency;
    this.pump();
  }

  /** Register this body's next page and ask for it, continuing from `offset`
   *  bytes that are already rendered. Registration and the request are one
   *  call, so a page can never start from a byte the loader has not set yet.
   *
   *  `visible` is the viewport state at registration. A new body carries it
   *  into its first page, so an on-screen body is never served behind the
   *  background work its own registration queued up behind. */
  read(id: string, offset: number, run: Task['run'], visible = false): void {
    const existing = this.tasks.get(id);
    if (existing) {
      existing.attached = true;
      existing.offset = offset;
      if (existing.run === run) {
        // The loader registered the same page again — after a page settled, this
        // is what asks for the continuation, so it re-enters the queue at the
        // signals this body currently holds.
        this.reorder(id);
        return;
      }
      existing.run = run;
      if (existing.running) existing.next = run;
      this.reorder(id);
      return;
    }
    this.tasks.set(id, {
      abort: undefined,
      attached: true,
      auto: true,
      enqueuedAt: this.served,
      failed: false,
      next: undefined,
      offset,
      order: (this.order += 1),
      promoted: visible,
      promotions: visible ? 1 : 0,
      run,
      running: false,
      visible,
      waiting: false,
    });
    this.enqueue(id);
  }

  /** Unmount: an in-flight page is aborted and its result ignored. */
  remove(id: string): void {
    const task = this.tasks.get(id);
    if (!task) return;
    task.attached = false;
    task.abort?.abort();
    this.tasks.delete(id);
    this.dequeue(id);
  }

  /** The body is on screen (true) or left the viewport (false). Entering the
   *  viewport is a fresh promotion for the next page; leaving it withdraws a
   *  promotion that has not started a page yet, so off-screen work cannot keep
   *  outranking the body the user is looking at and starve it. */
  setVisible(id: string, visible: boolean): void {
    const task = this.tasks.get(id);
    if (!task) return;
    const entering = visible && !task.visible;
    task.visible = visible;
    if (visible) {
      task.promoted = true;
      if (entering) task.promotions += 1;
    } else task.promoted = false;
    this.reorder(id);
  }

  /** Try the failed page again, as a fresh request from the viewport. */
  retry(id: string): void {
    const task = this.tasks.get(id);
    if (!task) return;
    task.failed = false;
    task.promoted = true;
    task.promotions += 1;
    this.reorder(id);
  }

  get activeCount(): number {
    return this.active;
  }

  get queuedCount(): number {
    return this.queue.length;
  }

  /** Re-sort one waiting task by its current signals. A task that is running
   *  has nothing to sort yet: it takes its place when it settles. */
  private reorder(id: string): void {
    const task = this.tasks.get(id);
    if (!task || task.running) {
      this.pump();
      return;
    }
    this.dequeue(id);
    this.enqueue(id);
  }

  private dequeue(id: string): void {
    const index = this.queue.indexOf(id);
    if (index >= 0) this.queue.splice(index, 1);
    const task = this.tasks.get(id);
    if (task) task.waiting = false;
  }

  /** Promoted work first, then bodies that are already being read, then bodies
   *  nobody has asked for yet — and within a band the oldest request wins, so
   *  nothing is starved. A request that has watched a full burst of pages go by
   *  while it waited outranks fresh work regardless of band, so screen-first
   *  priority cannot turn into indefinite starvation of everything else. */
  private enqueue(id: string): void {
    const task = this.tasks.get(id);
    if (!task || task.waiting || task.failed) return;
    const rank = (candidate: string): number => {
      const other = this.tasks.get(candidate);
      if (!other) return 3;
      if (other.visible || other.promoted) return 0;
      if (other.auto) return 1;
      return 2;
    };
    const beats = (candidate: string): boolean => {
      const other = this.tasks.get(candidate);
      if (!other) return true;
      // Bounded progress: `mine` is how many pages this request has waited through,
      // `theirs` the same for the candidate. A request that has waited a full burst
      // goes ahead of one that has not, whichever band either is in; between two
      // requests of the same age the bands decide as before.
      const mine = this.served - task.enqueuedAt >= BURST_PAGES;
      const theirs = this.served - other.enqueuedAt >= BURST_PAGES;
      if (mine !== theirs) return mine;
      const rankMine = rank(id);
      const rankTheirs = rank(candidate);
      if (rankMine !== rankTheirs) return rankTheirs > rankMine;
      return other.order > task.order;
    };
    const index = this.queue.findIndex(beats);
    if (index < 0) this.queue.push(id);
    else this.queue.splice(index, 0, id);
    task.waiting = true;
    // Entering (or re-entering) the queue restarts the wait: the age measures an
    // uninterrupted wait, so a body that keeps asking for its next page is never
    // treated as an old request.
    task.enqueuedAt = this.served;
    this.pump();
  }

  private pump(): void {
    while (this.active < this.concurrency) {
      const index = this.queue.findIndex((id) => this.tasks.get(id)?.running === false);
      if (index < 0) return;
      const id = this.queue.splice(index, 1)[0]!;
      const task = this.tasks.get(id)!;
      task.waiting = false;
      this.start(id, task);
    }
  }

  private start(id: string, task: Task): void {
    const abort = new AbortController();
    task.abort = abort;
    task.running = true;
    this.served += 1;
    // Only a page that was actually promoted spends the promotion; a promotion
    // that arrived while this page was already in flight is kept for the next.
    const promoted = task.visible || task.promoted;
    const promotions = task.promotions;
    this.active += 1;
    const page = task.run;
    const offset = task.offset;
    task.next = undefined;
    // Settling on a microtask keeps the slot counted for the whole synchronous
    // part of the next task, so the bound holds even when a page resolves at
    // once.
    const settle = (): void => {
      void Promise.resolve().then(() => this.settle(id, task, promoted, promotions, false));
    };
    const fail = (): void => {
      void Promise.resolve().then(() => this.settle(id, task, promoted, promotions, true));
    };
    void page(abort.signal, offset).then(settle, fail);
  }

  private settle(
    id: string,
    task: Task,
    promoted: boolean,
    promotions: number,
    failed: boolean,
  ): void {
    this.active -= 1;
    if (this.tasks.get(id) === task) {
      task.abort = undefined;
      task.running = false;
      task.failed = failed;
      if (promoted) {
        // The promotion this page started with is spent. `visible` is viewport
        // truth and is left to the observer; a promotion that arrived while the
        // page was in flight is kept for the next one.
        if (task.promotions === promotions) task.promoted = false;
        task.auto = false;
      }
      if (!task.attached) {
        this.tasks.delete(id);
        this.dequeue(id);
      } else if (!failed && task.next !== undefined) {
        // The loader registered a page while this one was in flight, so there
        // really is more text to read.
        task.next = undefined;
        task.auto = true;
        this.enqueue(id);
      }
    }
    this.pump();
  }
}

/** The one queue every message body shares. */
export const messageTextQueue = new MessageTextQueue();

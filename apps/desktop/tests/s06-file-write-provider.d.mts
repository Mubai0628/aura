export type ProviderRequest = { turn: string; step: number; tools: unknown[]; messages: unknown[] };
export type ProviderAction = { tool: string | null; arguments: Record<string, unknown> };
export function writeProvider(options: {
  path: string;
  text: string;
  observeRequest?: ((request: { authorization: string | undefined; body: unknown }) => void) | null;
  planAction?: ((request: ProviderRequest, action: ProviderAction) => ProviderAction) | null;
  maintenanceOutput?: import('@aura/contracts/generated').MemoryMaintenanceOutput;
  streamArguments?: { chunks: number; every: number } | null;
  finalText?: string;
}): Promise<{
  url: string;
  requests: ProviderRequest[];
  errors: string[];
  maintenanceRequests: unknown[];
  setAction(tool: string | null, argumentsValue: Record<string, unknown>): void;
  holdNext(before?: string): () => void;
  delayHeadersNext(ms: number): void;
  setFinalText(text: string): void;
  close(): Promise<void>;
}>;

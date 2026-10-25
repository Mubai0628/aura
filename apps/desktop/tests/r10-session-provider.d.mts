export interface FixtureProvider {
  url: string;
  errors: string[];
  requests: unknown[];
  close(): Promise<void>;
}

export function fixtureProvider(options?: {
  noTool?: boolean;
  long?: boolean;
  observeRequest?: ((request: { authorization: string | undefined; body: unknown }) => void) | null;
}): Promise<FixtureProvider>;

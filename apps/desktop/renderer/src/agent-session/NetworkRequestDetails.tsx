/** Header values that authenticate a caller; their names stay visible, their values never do. */
const SENSITIVE_HEADERS = new Set([
  'authorization',
  'proxy-authorization',
  'cookie',
  'cookie2',
  'set-cookie',
  'x-api-key',
  'api-key',
  'x-auth-token',
  'x-access-token',
]);
const BODY_PREVIEW_CHARS = 2000;

interface RequestFacts {
  method: string;
  url?: string;
  query?: string;
  headers: { name: string; value: string }[];
  body?: { text: string; total: number };
  credential?: string;
}

function bodyText(body: unknown): string | undefined {
  if (typeof body !== 'object' || body === null) return undefined;
  const value = body as Record<string, unknown>;
  if (value.kind === 'text' && typeof value.text === 'string') return value.text;
  if (value.kind === 'json') return JSON.stringify(value.value ?? null, null, 2);
  return undefined;
}

/** What the request really sends, read from the model action Core froze for this approval. */
function requestFacts(kind: string, action: Record<string, unknown>): RequestFacts {
  const method =
    kind === 'http.request' && typeof action.method === 'string'
      ? action.method.toUpperCase()
      : 'GET';
  const headers = Array.isArray(action.headers)
    ? action.headers.flatMap((header: unknown) => {
        if (typeof header !== 'object' || header === null) return [];
        const { name, value } = header as Record<string, unknown>;
        if (typeof name !== 'string' || typeof value !== 'string') return [];
        return [
          { name, value: SENSITIVE_HEADERS.has(name.trim().toLowerCase()) ? '（已隐藏）' : value },
        ];
      })
    : [];
  const body = bodyText(action.body);
  return {
    method,
    headers,
    ...(typeof action.url === 'string' ? { url: action.url } : {}),
    ...(kind === 'web.search' && typeof action.query === 'string' ? { query: action.query } : {}),
    ...(body === undefined
      ? {}
      : { body: { text: body.slice(0, BODY_PREVIEW_CHARS), total: body.length } }),
    ...(typeof action.credential === 'string' ? { credential: action.credential } : {}),
  };
}

/** Collapsed by default: the summary stays short, the full request is one click away. */
export function NetworkRequestDetails({
  kind,
  action,
}: {
  kind: string;
  action: Record<string, unknown>;
}) {
  const facts = requestFacts(kind, action);
  return (
    <details>
      <summary>查看请求内容</summary>
      <dl>
        <dt>方法</dt>
        <dd>{facts.method}</dd>
        {facts.url === undefined ? null : (
          <>
            <dt>完整地址</dt>
            <dd>{facts.url}</dd>
          </>
        )}
        {facts.query === undefined ? null : (
          <>
            <dt>搜索内容</dt>
            <dd>{facts.query}</dd>
          </>
        )}
        <dt>请求头</dt>
        <dd>
          {facts.headers.length ? (
            <ul>
              {facts.headers.map((header, index) => (
                <li key={`${index}:${header.name}`}>
                  {header.name}: {header.value}
                </li>
              ))}
            </ul>
          ) : (
            '无自定义请求头'
          )}
        </dd>
        <dt>请求正文</dt>
        <dd>
          {facts.body ? (
            <>
              <pre>{facts.body.text}</pre>
              {facts.body.total > facts.body.text.length ? (
                <p>
                  正文共 {facts.body.total} 个字符，这里只显示前 {BODY_PREVIEW_CHARS} 个。
                </p>
              ) : null}
            </>
          ) : (
            '无正文'
          )}
        </dd>
        {facts.credential === undefined ? null : (
          <>
            <dt>凭据</dt>
            <dd>
              使用已配置的工具凭据「{facts.credential}」；密钥由 Aura 在发送时加入，不在此显示。
            </dd>
          </>
        )}
      </dl>
    </details>
  );
}

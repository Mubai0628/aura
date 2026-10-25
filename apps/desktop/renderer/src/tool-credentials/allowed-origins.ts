/** Origins are user-entered destinations, never inferred from a model request. */
export function allowedOrigins(text: string): string[] {
  const origins = text
    .split(/[\s,]+/u)
    .filter(Boolean)
    .map((value) => {
      const url = new URL(value);
      if (
        !['http:', 'https:'].includes(url.protocol) ||
        url.username ||
        url.password ||
        url.search ||
        url.hash ||
        (url.pathname !== '/' && url.pathname !== '')
      )
        throw new Error(
          '允许网站仅接受 http 或 https 的来源（协议、主机和端口），不含路径、账号或查询参数。',
        );
      return url.origin;
    });
  const result = [...new Set(origins)].sort();
  if (result.length > 32) throw new Error('最多配置 32 个允许来源。');
  return result;
}

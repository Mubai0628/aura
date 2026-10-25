import { createServer } from 'node:http';

export async function httpService() {
  const requests: Array<{ url: string; authorization: string | undefined }> = [];
  const server = createServer((req, res) => {
    requests.push({ url: req.url ?? '/', authorization: req.headers.authorization });
    const body =
      req.url === '/download'
        ? Buffer.from('S07 real download')
        : req.url === '/page'
          ? Buffer.from('<html><body>S07 cached document for local reading.</body></html>')
          : Buffer.from('{"marker":"s07-service"}');
    res.writeHead(200, {
      'content-type':
        req.url === '/download'
          ? 'application/octet-stream'
          : req.url === '/page'
            ? 'text/html'
            : 'application/json',
      'content-length': body.length,
    });
    res.end(body);
  });
  await new Promise<void>((done) => server.listen(0, '127.0.0.1', done));
  const address = server.address();
  if (!address || typeof address === 'string') throw new Error('local HTTP port missing');
  return {
    url: `http://127.0.0.1:${address.port}`,
    requests,
    close: () =>
      new Promise<void>((done, reject) =>
        server.close((error) => (error ? reject(error) : done())),
      ),
  };
}

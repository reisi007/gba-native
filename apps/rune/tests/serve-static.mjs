import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import process from 'node:process';

const root = path.resolve(process.argv[2] ?? '');
if (!process.argv[2]) throw new Error('usage: node serve-static.mjs DIST_DIR');
const mime = new Map([
  ['.css', 'text/css; charset=utf-8'],
  ['.html', 'text/html; charset=utf-8'],
  ['.js', 'text/javascript; charset=utf-8'],
  ['.wasm', 'application/wasm'],
]);

const server = createServer(async (request, response) => {
  let relative;
  try {
    relative = decodeURIComponent(new URL(request.url ?? '/', 'http://localhost').pathname).slice(1);
  } catch {
    response.writeHead(400).end('bad request');
    return;
  }
  const file = path.resolve(root, relative || 'index.html');
  if (file !== root && !file.startsWith(`${root}${path.sep}`)) {
    response.writeHead(403).end('forbidden');
    return;
  }
  try {
    const body = await readFile(file);
    response.writeHead(200, {
      'content-type': mime.get(path.extname(file)) ?? 'application/octet-stream',
      'cache-control': 'no-store',
    }).end(body);
  } catch {
    response.writeHead(404).end('not found');
  }
});

server.listen(0, '127.0.0.1', () => {
  const address = server.address();
  if (!address || typeof address === 'string') throw new Error('could not read server address');
  console.log(`listening on http://127.0.0.1:${address.port}`);
});
process.on('SIGTERM', () => server.close(() => process.exit(0)));

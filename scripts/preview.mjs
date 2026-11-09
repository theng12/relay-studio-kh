// Disposable loopback fixture server only; never production transport.
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { randomUUID } from 'node:crypto';

const dist = resolve(import.meta.dirname, '../dist');
const port = Number(process.env.PORT || 7896);
const files = { 'index.html': 'text/html', 'relay-pane.js': 'text/javascript', 'bootstrap.mjs': 'text/javascript' };
const workers = [{ worker_id: 'worker_fixture_video', kind: 'video', has_live_token: true, online: true, last_seen_at: Date.now() / 1000, credits: { value: 11, source: 'flow.nzlxg', observedAt: new Date().toISOString() }, extension_version: '1.3.1', queue_depth: 0 }];
let errorStatus = 0;
let holdMint = false;
let releaseMint;
let mintRequests = 0;
const send = (response, status, data) => {
  response.writeHead(status, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' });
  response.end(JSON.stringify(data));
};
async function body(request) {
  let text = '';
  for await (const chunk of request) {
    text += chunk;
    if (Buffer.byteLength(text) > 8192) throw new Error('Fixture request exceeded 8 KB.');
  }
  return text ? JSON.parse(text) : {};
}
const server = createServer(async (request, response) => {
  const url = new URL(request.url, 'http://127.0.0.1');
  try {
    if (url.pathname === '/' || url.pathname === '/relay') { response.writeHead(302, { Location: '/relay/' }); return response.end(); }
    if (request.method === 'POST' && url.pathname === '/__preview/control') {
      const data = await body(request);
      errorStatus = [0, 401, 403, 404, 503].includes(data.errorStatus) ? data.errorStatus : 0;
      if (typeof data.holdMint === 'boolean') holdMint = data.holdMint;
      if (!holdMint && releaseMint) { releaseMint(); releaseMint = undefined; }
      return send(response, 200, { ok: true, mintRequests });
    }
    if (url.pathname.startsWith('/admin/api/extension-workers')) {
      if (errorStatus) return send(response, errorStatus, { detail: `Fixture Gateway refusal (${errorStatus}).` });
      if (request.method === 'GET' && url.pathname === '/admin/api/extension-workers') return send(response, 200, { preview: true, enabled: true, max_result_bytes: 268435456, workers });
      if (request.method === 'POST' && url.pathname === '/admin/api/extension-workers') {
        mintRequests++;
        const data = await body(request);
        if (!['video', 'image', 'audio'].includes(data.kind) || typeof data.worker_id !== 'string' || !data.worker_id.trim() || data.worker_id.length > 120) return send(response, 400, { detail: 'A valid extension kind and worker ID are required.' });
        if (holdMint) await new Promise((resolve) => { releaseMint = resolve; });
        let worker = workers.find((entry) => entry.worker_id === data.worker_id);
        if (!worker) { worker = { worker_id: data.worker_id, kind: data.kind, online: false, last_seen_at: null, credits: null, extension_version: null, queue_depth: null }; workers.push(worker); }
        worker.has_live_token = true;
        return send(response, 200, { ok: true, worker_id: data.worker_id, token: `fixture-only-${randomUUID()}` });
      }
      const match = url.pathname.match(/^\/admin\/api\/extension-workers\/([^/]+)$/);
      if (request.method === 'DELETE' && match) {
        const worker = workers.find((entry) => entry.worker_id === decodeURIComponent(match[1]));
        if (!worker) return send(response, 404, { detail: 'Worker not found.' });
        worker.has_live_token = false;
        worker.online = false;
        return send(response, 200, { ok: true, revoked: 1 });
      }
      return send(response, 404, { detail: 'Fixture route not found.' });
    }
    const name = url.pathname === '/relay/' ? 'index.html' : url.pathname.replace(/^\/relay\//, '');
    if (request.method !== 'GET' || !Object.hasOwn(files, name)) return send(response, 404, { detail: 'Not found.' });
    const content = await readFile(resolve(dist, name));
    response.writeHead(200, { 'Content-Type': `${files[name]}; charset=utf-8`, 'Cache-Control': 'no-store', 'Content-Security-Policy': "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self'; frame-ancestors 'self'; base-uri 'none'; form-action 'self'" });
    response.end(content);
  } catch (error) { send(response, 400, { detail: error.message }); }
});
server.listen(port, '127.0.0.1', () => console.info(`Relay fixture preview ready at http://127.0.0.1:${port}/relay/`));

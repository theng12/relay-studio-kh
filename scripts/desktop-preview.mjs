// Disposable browser fixture only: no native runtime, OS changes or Gateway traffic.
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
const assets = new URL('../desktop-dist/', import.meta.url);
const bridge = `
const snapshot = { settings: { gateway_url: null, start_at_login: true }, autostart_observed: false, autostart_error: 'Your saved choice is on, but macOS reports Start at Login is off. Check it in System Settings.' };
window.__relayFixture = { calls: [], snapshot };
window.__TAURI__ = { event: { listen: async () => () => {} }, core: { invoke: async (name, args) => {
  window.__relayFixture.calls.push({ name, args });
  if (name === 'get_settings') return structuredClone(snapshot);
  if (name === 'save_settings') {
    if (args.gatewayUrl) {
      const url = new URL(args.gatewayUrl);
      if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password || url.pathname !== '/' || url.search || url.hash) throw Error('Use a Gateway origin without credentials, path, query or fragment.');
      snapshot.settings.gateway_url = url.origin;
    } else snapshot.settings.gateway_url = null;
    if (args.startAtLogin !== null) {
      snapshot.settings.start_at_login = args.startAtLogin;
      snapshot.autostart_observed = args.startAtLogin;
      snapshot.autostart_error = null;
    }
    return null;
  }
  if (name === 'gateway_health') return { configured: !!snapshot.settings.gateway_url, reachable: false, origin: snapshot.settings.gateway_url };
  if (name === 'open_gateway' || name === 'quit_app') return null;
  throw Error('Unknown fixture command');
} } };
`;
const files = new Map([['/', ['index.html', 'text/html']], ['/app.mjs', ['app.mjs', 'text/javascript']], ['/app-info.mjs', ['app-info.mjs', 'text/javascript']], ['/recovery.mjs', ['recovery.mjs', 'text/javascript']], ['/desktop.css', ['desktop.css', 'text/css']]]);
createServer(async (req, res) => {
  res.setHeader('Cache-Control', 'no-store');
  if (req.url === '/fixture-bridge.js') { res.setHeader('Content-Type', 'text/javascript'); res.end(bridge); return; }
  if (req.url === '/favicon.ico') { res.writeHead(204).end(); return; }
  const entry = files.get(req.url);
  if (!entry) { res.writeHead(404).end(); return; }
  try {
    let body = await readFile(new URL(entry[0], assets), 'utf8');
    if (entry[0] === 'index.html') body = body.replace('<script type="module"', '<script src="./fixture-bridge.js"></script><script type="module"').replace('<body>', '<body><p style="color:#e7edf6;background:#151b24;padding:12px;font:13px system-ui">Browser fixture — fake native bridge. No Mac login settings, saved files or Gateway connection are changed.</p>');
    res.setHeader('Content-Type', entry[1]); res.end(body);
  } catch { res.writeHead(500).end('Prepare desktop web assets first.'); }
}).listen(7920, '127.0.0.1', () => console.info('Desktop browser fixture ready at http://127.0.0.1:7920; native behavior unverified.'));

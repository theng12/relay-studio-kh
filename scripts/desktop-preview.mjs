// Disposable browser fixture only. Never bundled: no Chrome, Keychain or Gateway actions.
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
const assets = new URL('../desktop-dist/', import.meta.url);
const bridge = `
const settings = { settings: { gateway_url: null, start_at_login: false, appearance: 'system' }, autostart_observed: false, autostart_error: null };
const manager = {
  profiles: [],
  memory: { physical_mb: 32768, available_mb: 12288, budget_mb: 4096, used_mb: 0, per_profile_mb: 1024, queued: 0, reason: 'Fixture measurements only; no real processes are running.' },
  settings: { memory_budget_mb: 4096, reserve_mb: 2048, max_active: null, pause_min_seconds: 5, pause_max_seconds: 15 },
  support: { native: true, chrome_available: true, telemetry_authorized: false, telemetry_error: 'Fixture: no Gateway manager token paired.', pacing_applied: false, pacing_error: 'Fixture: desired pacing is not an extension acknowledgement.' }
};
function add(labels) {
  for (const label of labels) {
    const n = manager.profiles.length + 1;
    manager.profiles.push({id:'fixture-profile-'+n, number:n, label:String(label), enabled:true, desired_open:false, process_state:'closed', process_id:null, memory_mb:null, close_pending:false, error:null, attention:n===2, gateway:{connection:'unknown',sign_in:n===2?'signed_out':'unknown',readiness:'unknown',activity:'unknown',provider:null,account_id:null,image_worker_id:'',video_worker_id:'',observed_at:null,credits:null,next_eligible_at:null}});
  }
}
add(['Image and video 1','Needs sign-in','Image and video 3','Image and video 4','Image and video 5']);
window.__relayFixture = { calls: [], snapshot: settings, manager };
window.__TAURI__ = { event: { listen: async () => () => {} }, core: { invoke: async (name,args={}) => {
  const recorded = {...args}; if ('token' in recorded) recorded.token = '[redacted]';
  window.__relayFixture.calls.push({name,args:recorded});
  if (name === 'get_settings') return structuredClone(settings);
  if (name === 'save_settings') {
    if (args.gatewayUrl) {
      const url = new URL(args.gatewayUrl);
      if (!['http:','https:'].includes(url.protocol) || url.username || url.password || url.pathname !== '/' || url.search || url.hash) throw Error('Use a Gateway origin without credentials, path, query or fragment.');
      settings.settings.gateway_url = url.origin;
    } else settings.settings.gateway_url = null;
    if (args.accountsView) settings.settings.accounts_view = args.accountsView;
    if (args.appearance) settings.settings.appearance = args.appearance;
    if (args.startAtLogin != null) { settings.settings.start_at_login = args.startAtLogin; settings.autostart_observed = args.startAtLogin; }
    return null;
  }
  if (name === 'gateway_health') return {configured:!!settings.settings.gateway_url,reachable:false,origin:settings.settings.gateway_url};
  if (name === 'get_extension_setup') {
    const text = await (await fetch('./extension-info.mjs')).text();
    const extensions = JSON.parse(text.slice('export default '.length).trim().replace(/;$/,'')).extensions;
    return {browsers:[{id:'chrome',name:'Google Chrome (fixture)',detected:true,instructions:'Fixture only. Manually load reviewed extension folders in the exact managed Chrome profile.'}],extensions};
  }
  if (['open_gateway','quit_app','open_extension_settings'].includes(name)) return null;
  if (name === 'manager_add_profiles') add(args.labels);
  else if (name === 'manager_delete_profile') {
    const at = manager.profiles.findIndex(p=>p.id===args.profileId); if(at<0) throw Error('Unknown fixture profile.');
    manager.profiles.splice(at,1);
  }
  else if (name === 'manager_update_profile') {
    const p=manager.profiles.find(p=>p.id===args.profileId); if(!p) throw Error('Unknown fixture profile.');
    for (const [key,field] of [['label','label'],['enabled','enabled'],['imageWorkerId','image_worker_id'],['videoWorkerId','video_worker_id']]) {
      if(args[key] !== undefined && args[key] !== null) { if(field.endsWith('_worker_id')) p.gateway[field]=args[key]; else p[field]=args[key]; }
    }
  } else if (name === 'manager_open_profiles') {
    for (const p of manager.profiles.filter(p=>args.profileIds.includes(p.id))) {
      if(!p.enabled) {p.error='This saved account is off.';continue;}
      p.desired_open=true;p.close_pending=false;
      const measuredSlots=Math.max(0,Math.floor((Math.min(manager.memory.available_mb,manager.settings.memory_budget_mb??manager.memory.available_mb)-manager.settings.reserve_mb)/manager.memory.per_profile_mb));
      const slots=manager.settings.max_active==null?measuredSlots:Math.min(measuredSlots,manager.settings.max_active);
      p.process_state=manager.profiles.filter(q=>q.process_state==='open'&&q.id!==p.id).length<slots?'open':'queued';
      p.process_id=p.process_state==='open'?10000+p.number:null;
      p.memory_mb=p.process_state==='open'?manager.memory.per_profile_mb:null;
    }
  } else if (name === 'manager_show_profiles') {
    for (const id of args.profileIds) {
      const p=manager.profiles.find(p=>p.id===id);
      if(!p||p.process_state!=='open'||!p.process_id) throw Error('Fixture: no verified resident profile to show.');
    }
  } else if (name === 'manager_close_profiles') {
    for (const p of manager.profiles.filter(p=>args.profileIds.includes(p.id))) {
      p.desired_open=false;
      p.close_pending=p.process_state==='open';
      if(p.close_pending) p.error='Close requested; waiting for fresh Gateway pause and safe-close acknowledgements.';
      else {p.process_state='closed';p.error=null;}
    }
  } else if (name === 'manager_save_settings') {
    manager.settings={memory_budget_mb:args.memoryBudgetMb,reserve_mb:args.reserveMb,max_active:args.maxActive,pause_min_seconds:args.pauseMinSeconds,pause_max_seconds:args.pauseMaxSeconds};
  } else if (name === 'manager_pair_gateway') {
    throw Error('Fixture only: pairing is not performed. No token was stored or sent.');
  } else if (name !== 'manager_snapshot') throw Error('Unknown fixture command: '+name);
  manager.memory.used_mb=manager.profiles.reduce((sum,p)=>sum+(p.memory_mb??0),0);
  manager.memory.queued=manager.profiles.filter(p=>p.process_state==='queued').length;
  return structuredClone(manager);
} } };
`;
const files = new Map([
  ['/', ['index.html','text/html']],
  ...['app.mjs','accounts.mjs','app-info.mjs','recovery.mjs','setup.mjs','extension-info.mjs','browsers.mjs','overview.mjs','shell.mjs'].map(name=>['/'+name,[name,'text/javascript']]),
  ['/desktop.css',['desktop.css','text/css']],
  ['/relay-mark.png',['relay-mark.png','image/png']],
  // Same-origin harness for render acceptance (present only while a render file exists).
  ['/render.html',['render.html','text/html']]
]);
const port = Number(process.env.PORT || 7921);
createServer(async (req,res) => {
  res.setHeader('Cache-Control','no-store');
  if(req.url==='/fixture-bridge.js') {res.setHeader('Content-Type','text/javascript');res.end(bridge);return;}
  if(req.url==='/favicon.ico') {res.writeHead(204).end();return;}
  const entry=files.get(req.url); if(!entry) {res.writeHead(404).end();return;}
  try {
    let body=await readFile(new URL(entry[0],assets), entry[1]==='image/png' ? undefined : 'utf8');
    if(entry[0]==='index.html') body=body.replace('<script type="module"','<script src="./fixture-bridge.js"></script><script type="module"').replace('<body>','<body><p style="color:#e7edf6;background:#151b24;padding:12px;font:13px system-ui;margin:0">Browser fixture — synthetic accounts and memory. No Chrome processes, Keychain items, saved login preferences or Gateway jobs are changed.</p>');
    res.setHeader('Content-Type',entry[1]);res.end(body);
  } catch {res.writeHead(500).end('Prepare desktop web assets first.');}
}).listen(port,'127.0.0.1',()=>console.info('Desktop browser fixture ready at http://127.0.0.1:'+port+'; real native behavior is unverified.'));

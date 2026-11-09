import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { createContext, SourceTextModule, SyntheticModule } from 'node:vm';
import { webcrypto } from 'node:crypto';
import { videoRoot, videoModule } from './source.mjs';

const source = await readFile(resolve(videoRoot, 'src/background.js'), 'utf8');
const rules = await videoModule('video-rules.js');
export const worker = await videoModule('gateway-worker.js');
export const JOB = { jobId: 'fixture_1', kind: 'video', videoMode: 'text', prompt: 'Fixture', modelKey: 'fixture_model', modelLabel: 'Fixture model', durationSec: 4, aspectRatio: '16:9', flowProjectId: 'fixture_project' };

// Evaluate real or isolated patched source without access to a Chrome profile.
export async function background({ sourceText = source, flow = async () => { throw new Error('Unexpected Flow operation'); }, resultError } = {}) {
  const stored = {};
  const reports = [];
  const settings = { mode: 'studio', gatewayUrl: 'http://relay.invalid', gatewayWorkerId: 'worker_fixture', flowWorkspaceUrl: 'https://flow.google.com', providerCooldown: { autoResume: false }, humanDelayMinSec: 0, humanDelayMaxSec: 0 };
  let claim = null;
  const copy = (value) => structuredClone(value);
  const event = { addListener() {}, removeListener() {} };
  const chrome = {
    storage: { local: {
      async get(key) { return { [key]: copy(stored[key]) }; },
      async set(values) { Object.assign(stored, copy(values)); },
      async remove(keys) { for (const key of [].concat(keys)) delete stored[key]; },
    } },
    runtime: { onMessage: event },
    alarms: { create() {}, clear() {}, onAlarm: event },
    tabs: {
      async query() { return [{ id: 1, status: 'complete', url: 'https://flow.google.com/project/fixture_project' }]; },
      async get() { return { id: 1, url: 'https://flow.google.com/project/fixture_project' }; },
    },
    scripting: { async executeScript({ args }) { return [{ result: { ok: true, result: await flow(args[0]) } }]; } },
  };
  const context = createContext({ chrome, crypto: webcrypto, console, setTimeout, clearTimeout, URL, Uint8Array,
    fetch: async () => new Response(Uint8Array.from([1, 2, 3]), { headers: { 'Content-Type': 'video/mp4' } }),
  });
  const deps = {
    './lib/config.js': {
      getSettings: async () => copy(settings), saveSettings: async (patch) => Object.assign(settings, patch),
      normalizeFlowProjectId: String, flowWorkspace: () => ({ origin: 'https://flow.google.com', match: 'https://flow.google.com/*' }),
      getFleetCredentials: async () => { throw new Error('Fleet not part of test'); }, getGatewayToken: async () => 'fixture-only',
    },
    './lib/story-studio.js': { StudioClient: class { constructor() {} } },
    './lib/flow-exec.js': { flowCommand() { throw new Error('Real Flow execution prohibited'); } },
    './lib/video-models.js': {
      inventoryResult: () => ({ models: [] }), resolveVideoModel: () => ({ label: JOB.modelLabel }), snapshotVideoModel() { throw new Error('Unexpected local enqueue'); },
    },
    './lib/video-rules.js': rules,
    './lib/gateway-worker.js': worker,
    './lib/gateway-client.js': {
      claimGatewayJob: async () => copy(claim), testGateway: async () => ({ ok: true }),
      reportGatewayProgress: async (...args) => reports.push(['progress', ...args.slice(1)]),
      reportGatewayResult: async (...args) => { if (resultError) throw resultError; reports.push(['result', ...args.slice(1)]); },
      reportGatewayFailure: async (...args) => reports.push(['fail', ...args.slice(1)]),
    },
  };
  const module = new SourceTextModule(`${sourceText}\nexport { gatewayTick, processQueue, generateAndSave, recoverInterruptedJobs };`, { context });
  await module.link((specifier) => {
    const values = deps[specifier];
    assert.ok(values, `Unmocked dependency: ${specifier}`);
    return new SyntheticModule(Object.keys(values), function () {
      for (const [key, value] of Object.entries(values)) this.setExport(key, value);
    }, { context });
  });
  await module.evaluate();
  await new Promise(setImmediate);
  return { api: module.namespace, settings, stored, reports, setClaim(value) { claim = value; } };
}

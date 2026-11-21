import { readWorkers, mintWorker, revokeWorker, reportedCredit, workerState } from './api.mjs';

// Per-image price for the image channel (owner decision 2026-11-21): it starts at
// 0 credits — a real price, never "unknown" — and is adjustable per channel. It
// rides the account/quote payload Relay gives Gateway. shortcut: the value is
// prepared but not sent yet; R5's image job is what proves the wire.
export const DEFAULT_IMAGE_CREDITS_PER_IMAGE = 0;
// Same storage path the connector pane already uses: its own origin's browser
// storage. No new store file, no migration. Falls back to memory when storage
// is unavailable (the value then just does not survive a reload).
const IMAGE_PRICE_KEY = 'relay-image-credits-per-image';
let imagePriceMemory;

// Blank and every invalid value mean free, matching the image extension's rule:
// an unset price is free, not unpriced.
export function normalizeImageCreditsPerImage(value) {
  const number = Number(value);
  return Number.isFinite(number) && number >= 0 ? number : DEFAULT_IMAGE_CREDITS_PER_IMAGE;
}

export function storedImageCreditsPerImage() {
  try {
    const raw = globalThis.localStorage?.getItem(IMAGE_PRICE_KEY);
    if (raw === null || raw === undefined) return imagePriceMemory ?? DEFAULT_IMAGE_CREDITS_PER_IMAGE;
    return normalizeImageCreditsPerImage(raw);
  } catch {
    return imagePriceMemory ?? DEFAULT_IMAGE_CREDITS_PER_IMAGE;
  }
}

export function saveImageCreditsPerImage(value) {
  const next = normalizeImageCreditsPerImage(value);
  imagePriceMemory = next;
  try { globalThis.localStorage?.setItem(IMAGE_PRICE_KEY, String(next)); } catch { /* memory only */ }
  return next;
}

// The price portion of the account/quote payload Relay gives Gateway. Field names
// follow the image extension's quote builder (value/unit/source) so the payload
// needs no re-keying; provider, accountId, payloadSha256 and observedAt come from
// the live claim itself.
export function imageQuotePrice(credits) {
  return { value: normalizeImageCreditsPerImage(credits), unit: 'credits', source: 'settings.image-credits' };
}

export function mountRelayConnector(root, { request = fetch, canManage = false, appInfo } = {}) {
const KINDS = { video: 'Video Generator', image: 'Image Generator', audio: 'Audio Generator' };
const $ = (id) => root.querySelector(`#relay-${id}`);
const element = (tag, className, text) => {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
};
let pairingKind;
let revokingId;
let readSequence = 0;
let minting = false;
let revoking = false;

function timestamp(value) {
  if (!value) return 'Not reported';
  const date = new Date(value);
  return Number.isNaN(date.valueOf()) ? 'Time unavailable' : new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' }).format(date);
}
function detail(list, label, value) {
  list.append(element('dt', '', label), element('dd', '', value));
}
function beginPair(kind, workerId = '') {
  if (!canManage) return;
  pairingKind = kind;
  $('pair-title').textContent = `Connect ${KINDS[kind]}`;
  $('pair-form').reset();
  $('worker-id').value = workerId;
  $('pair-fields').hidden = false;
  $('token-view').hidden = true;
  $('pair-submit').hidden = false;
  $('copy-token').hidden = true;
  $('pair-error').hidden = true;
  $('pair-dialog').showModal();
  $('worker-id').focus();
}
function beginRevoke(worker) {
  if (!canManage) return;
  revokingId = worker.workerId;
  $('revoke-description').textContent = `${worker.label || KINDS[worker.kind]} · ${worker.workerId}`;
  $('revoke-error').hidden = true;
  $('revoke-dialog').showModal();
}

// The image channel's own price row, shown with its workers. Plain words, real units.
function imagePriceRow() {
  const section = element('article', 'worker image-price');
  const row = element('div', 'worker-row');
  const identity = element('div', 'worker-identity');
  identity.append(
    element('span', 'worker-title', 'Image price'),
    element('span', 'worker-subtitle', 'What one image costs. Free is 0.'),
  );
  const input = element('input', '', undefined);
  input.type = 'number';
  input.min = '0';
  input.step = '1';
  input.id = 'relay-image-price';
  input.value = String(storedImageCreditsPerImage());
  input.setAttribute('aria-label', 'Credits per image');
  input.addEventListener('input', () => saveImageCreditsPerImage(input.value));
  const unit = element('span', 'secondary', 'credits per image');
  row.append(identity, input, unit);
  section.append(row);
  return section;
}

function workerRow(kind, worker) {
  const section = element('article', 'worker');
  const row = element('div', 'worker-row');
  const identity = element('div', 'worker-identity');
  identity.append(element('span', 'worker-title', worker?.label || KINDS[kind]));
  identity.append(element('span', 'worker-subtitle', worker ? worker.workerId : 'No worker connected'));
  const status = worker ? workerState(worker) : { label: 'Not connected', tone: 'off' };
  const state = element('div', '', undefined);
  state.append(element('span', `state ${status.tone}`, status.label));
  if (worker?.telemetry?.queue?.paused === true) state.append(element('span', 'worker-subtitle', 'Queue paused'));
  const credit = element('div', 'worker-credit');
  const credits = worker?.telemetry?.credits;
  const value = reportedCredit(credits);
  credit.append(element('span', 'credit-label', 'Remaining credits'), element('span', 'credit-value', value === null ? 'Unknown' : new Intl.NumberFormat().format(value)));
  const action = element('button', '', worker && !worker.revoked ? 'Show details' : 'Connect');
  action.type = 'button';
  row.append(identity, state, credit, action);
  section.append(row);
  if (worker && !worker.revoked) {
    // A visible button owns the disclosure; no native <details> triangle (owner, 2026-11-12).
    const more = element('div', 'worker-details');
    more.hidden = true;
    more.id = `relay-worker-details-${worker.workerId}`;
    const list = element('dl');
    detail(list, 'Extension', KINDS[kind]);
    detail(list, 'Last reported', timestamp(worker.lastSeen));
    detail(list, 'Extension version', worker.telemetry?.extensionVersion || 'Not reported');
    detail(list, 'Credit source', credits?.source || 'Not reported');
    detail(list, 'Credits observed', timestamp(credits?.observedAt));
    const queue = worker.telemetry?.queue;
    detail(list, 'Queue', Number.isInteger(queue?.depth) && queue.depth >= 0 ? `${queue.depth} waiting${queue.paused === true ? ' · paused' : ''}` : 'Unknown');
    if (worker.currentJob) detail(list, 'Current job', `${worker.currentJob.jobId} · ${worker.currentJob.phase || 'Phase not reported'}`);
    if (worker.heldJobs?.length) detail(list, 'Held outcomes', worker.heldJobs.join(', '));
    const visible = worker.telemetry?.tab?.visible;
    detail(list, 'Provider tab visible', visible === true ? 'Yes' : visible === false ? 'No' : 'Unknown');
    more.append(list);
    const models = worker.telemetry?.capabilities?.models;
    if (Array.isArray(models)) {
      more.append(element('strong', '', 'Reported models'));
      const entries = element('ul', 'model-list');
      for (const model of models) entries.append(element('li', '', model.label || model.key || 'Unnamed reported model'));
      if (!models.length) entries.append(element('li', '', 'No models reported'));
      more.append(entries);
    }
    const revoke = element('button', '', 'Revoke access');
    revoke.type = 'button';
    revoke.addEventListener('click', () => beginRevoke(worker));
    if (canManage) more.append(revoke);
    action.setAttribute('aria-controls', more.id);
    action.addEventListener('click', () => {
      more.hidden = !more.hidden;
      action.textContent = more.hidden ? 'Show details' : 'Hide details';
      action.setAttribute('aria-expanded', String(!more.hidden));
    });
    action.setAttribute('aria-expanded', 'false');
    section.append(more);
  } else {
    action.disabled = !canManage;
    action.addEventListener('click', () => beginPair(kind, worker?.workerId));
  }
  return section;
}

function renderWorkers(workers) {
  const unknown = workers.find((worker) => !KINDS[worker.kind] || !worker.workerId);
  if (unknown) throw new Error('Gateway returned a worker with an unsupported kind or missing ID. Check its worker API version.');
  const rows = [];
  for (const kind of Object.keys(KINDS)) {
    // The price belongs to the image channel, so it is shown on that channel's rows.
    if (kind === 'image') rows.push(imagePriceRow());
    const matching = workers.filter((worker) => worker.kind === kind);
    if (matching.length) rows.push(...matching.map((worker) => workerRow(kind, worker)));
    else rows.push(workerRow(kind, null));
  }
  $('workers').replaceChildren(...rows);
}

async function refresh() {
  const sequence = ++readSequence;
  $('refresh').disabled = true;
  $('workers').setAttribute('aria-busy', 'true');
  $('readback').textContent = 'Reading Gateway…';
  try {
    const data = await readWorkers(request);
    if (sequence !== readSequence) return;
    if (data.enabled === false) $('workers').replaceChildren(element('p', 'loading', 'The worker connector is off in Gateway. Connection actions are unavailable until worker access is enabled.'));
    else renderWorkers(data.workers);
    $('limits').textContent = Number.isFinite(data.max_result_bytes) ? `Artifact upload limit: ${new Intl.NumberFormat().format(data.max_result_bytes / 1024 / 1024)} MiB.` : '';
    $('preview').hidden = data.preview !== true && root.dataset.preview !== 'true';
    $('error').hidden = true;
    $('readback').textContent = data.enabled === false ? 'Worker connector is off' : `Checked ${new Intl.DateTimeFormat(undefined, { timeStyle: 'short' }).format(new Date())}`;
  } catch (error) {
    if (sequence !== readSequence) return;
    $('error-text').textContent = error.message;
    $('login').hidden = error.status !== 401 && error.status !== 403;
    $('error').hidden = false;
    $('readback').textContent = 'Could not read Gateway';
    $('workers').replaceChildren(element('p', 'loading', 'Connection state is unavailable. Refresh after fixing the connection.'));
  } finally {
    if (sequence === readSequence) {
      $('refresh').disabled = false;
      $('workers').setAttribute('aria-busy', 'false');
    }
  }
}

$('gateway-url').textContent = location.origin;
$('management-note').hidden = canManage;
$('refresh').addEventListener('click', refresh);
$('pair-form').addEventListener('submit', async (event) => {
  event.preventDefault();
  if (!canManage || minting || !$('token-view').hidden) return;
  minting = true;
  $('pair-dialog').dataset.pending = 'true';
  $('pair-dialog').querySelector('.close').disabled = true;
  $('pair-submit').disabled = true;
  $('pair-error').hidden = true;
  const workerId = $('worker-id').value.trim();
  try {
    const data = await mintWorker({ workerId, kind: pairingKind }, request);
    $('worker-token').value = data.token;
    $('pair-worker').textContent = `Worker ID: ${workerId}`;
    $('pair-url').value = location.origin;
    $('pair-fields').hidden = true;
    $('token-view').hidden = false;
    $('pair-submit').hidden = true;
    $('copy-token').hidden = false;
    $('copy-status').textContent = 'The token is not saved in this browser.';
    await refresh();
  } catch (error) {
    $('pair-error').textContent = `${error.message} If the request reached Gateway, its token may already have been created. Check the worker record and revoke access before creating a replacement.`;
    await refresh();
    $('pair-error').hidden = false;
  } finally {
    minting = false;
    $('pair-dialog').dataset.pending = 'false';
    $('pair-dialog').querySelector('.close').disabled = false;
    $('pair-submit').disabled = false;
  }
});
$('copy-token').addEventListener('click', async () => {
  try {
    await navigator.clipboard.writeText($('worker-token').value);
    $('copy-status').textContent = 'Token copied. Paste it into the extension’s Gateway settings.';
  } catch {
    $('worker-token').type = 'text';
    $('worker-token').select();
    $('copy-status').textContent = 'Clipboard access is unavailable. Copy the selected token manually.';
  }
});
$('pair-dialog').addEventListener('close', () => {
  $('worker-token').value = '';
  $('worker-token').type = 'password';
  $('pair-form').reset();
});
$('revoke-form').addEventListener('submit', async (event) => {
  event.preventDefault();
  if (!canManage || revoking) return;
  revoking = true;
  $('revoke-dialog').dataset.pending = 'true';
  $('revoke-dialog').querySelector('.close').disabled = true;
  $('revoke-submit').disabled = true;
  $('revoke-error').hidden = true;
  try { await revokeWorker(revokingId, request); $('revoke-dialog').close(); await refresh(); }
  catch (error) { $('revoke-error').textContent = error.message; $('revoke-error').hidden = false; }
  finally {
    revoking = false;
    $('revoke-dialog').dataset.pending = 'false';
    $('revoke-dialog').querySelector('.close').disabled = false;
    $('revoke-submit').disabled = false;
  }
});
for (const dialog of root.querySelectorAll('dialog')) {
  dialog.querySelectorAll('.close').forEach((button) => button.addEventListener('click', () => { if (dialog.dataset.pending !== 'true') dialog.close(); }));
  dialog.addEventListener('cancel', (event) => { if (dialog.dataset.pending === 'true') event.preventDefault(); });
  if (dialog.id !== 'relay-revoke-dialog') dialog.addEventListener('click', (event) => { if (event.target === dialog && dialog.dataset.pending !== 'true') dialog.close(); });
}

async function readAppInfo() {
  try {
    const info = appInfo;
    if (!info) throw new Error('Version information could not be read.');
    $('version').textContent = `v${info.version} · What's New`;
    $('version').disabled = false;
    for (const release of info.releases) {
      $('release-notes').append(element('h3', '', `v${release.version}${release.version === info.version ? ' · Current' : ''}`));
      const list = element('ul');
      for (const item of release.items) list.append(element('li', '', item));
      $('release-notes').append(list);
    }
  } catch { $('version').textContent = 'Version unavailable'; }
}
$('version').addEventListener('click', () => $('whats-new').showModal());
readAppInfo();
refresh();

}

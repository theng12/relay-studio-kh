const ROOT = '/admin/api/extension-workers';

export async function gatewayRequest(path = '', body, method = body === undefined ? 'GET' : 'POST', request = fetch) {
  const response = await request(`${ROOT}${path}`, {
    method, credentials: 'same-origin', cache: 'no-store',
    headers: body === undefined ? {} : { 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  let data;
  try { data = await response.json(); }
  catch {
    const error = new Error(`Gateway returned an unreadable response (${response.status}). Try refreshing.`);
    error.status = response.status;
    throw error;
  }
  if (!response.ok) {
    const error = new Error(typeof data?.detail === 'string' ? data.detail : `Gateway request failed (${response.status}).`);
    error.status = response.status;
    throw error;
  }
  return data;
}

export async function readWorkers(request) {
  let data;
  try { data = await gatewayRequest('', undefined, 'GET', request); }
  catch (error) {
    if (error.status === 404) return { enabled: false, workers: [] };
    throw error;
  }
  if (!Array.isArray(data?.workers)) throw new Error('Gateway did not return a worker list. Check that its worker API is installed.');
  return { ...data, workers: data.workers.map((worker) => ({
    workerId: worker.worker_id, kind: worker.kind,
    revoked: worker.has_live_token === false,
    online: worker.online, lastSeen: typeof worker.last_seen_at === 'number' ? new Date(worker.last_seen_at * 1000).toISOString() : null,
    currentJob: worker.current_job, heldJobs: worker.held_jobs,
    telemetry: { extensionVersion: worker.extension_version, credits: worker.credits, queue: { depth: worker.queue_depth } },
  })) };
}

export async function mintWorker({ workerId, kind }, request) {
  const data = await gatewayRequest('', { worker_id: workerId, kind }, 'POST', request);
  if (typeof data?.token !== 'string' || !data.token) throw new Error('Gateway did not return a worker token. Check the worker record before trying again.');
  return data;
}

export const revokeWorker = (workerId, request) => gatewayRequest(`/${encodeURIComponent(workerId)}`, undefined, 'DELETE', request);

export function reportedCredit(credits) {
  const value = credits?.value;
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : null;
}

export function workerState(worker) {
  if (worker.revoked === true) return { label: 'Access revoked', tone: 'off' };
  if (worker.online === true) return { label: 'Connected', tone: 'ok' };
  if (worker.online === false && worker.lastSeen) return { label: 'Offline', tone: 'warn' };
  if (worker.online === false) return { label: 'Waiting for extension', tone: 'info' };
  return { label: 'Not checked', tone: 'info' };
}

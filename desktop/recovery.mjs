// Only transport health and opening an existing page are retried; never jobs or writes.
export function createRecovery({ probe, report }) {
  let running = false;
  let connectedOrigin;
  return async function tick() {
    if (running) return;
    running = true;
    try {
      const state = await probe();
      if (!state.configured) {
        connectedOrigin = undefined;
        report('Choose your Gateway address to connect.');
      } else if (!state.reachable) {
        connectedOrigin = undefined;
        report('Gateway is unavailable. Relay will check again; no work is resubmitted.');
      } else {
        // A typed address that works must never open a sign-in window on its own: opening
        // Gateway's own pages is an explicit action, not a side effect of saving.
        connectedOrigin = state.origin;
        report('Gateway responded. Gateway\'s own admin pages are opened separately, and they are not part of pairing.');
      }
    } catch {
      connectedOrigin = undefined;
      report('Could not check Gateway. Relay will check again; no work is resubmitted.');
    } finally { running = false; }
  };
}

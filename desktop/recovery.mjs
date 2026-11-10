// Only transport health and opening an existing page are retried; never jobs or writes.
export function createRecovery({ probe, openGateway, report }) {
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
        if (connectedOrigin !== state.origin) {
          await openGateway();
          connectedOrigin = state.origin;
        }
        report('Gateway responded. Reconnect opens its sign-in and worker interface.');
      }
    } catch {
      connectedOrigin = undefined;
      report('Could not check Gateway. Relay will check again; no work is resubmitted.');
    } finally { running = false; }
  };
}

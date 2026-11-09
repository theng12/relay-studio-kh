// Fixture preview only. Gateway supplies its existing app auth at mount time.
const root = document.querySelector('.relay-connector');
root.dataset.preview = 'true';
window.mountRelayConnector(root, { canManage: new URLSearchParams(location.search).get('readOnly') !== 'true' });

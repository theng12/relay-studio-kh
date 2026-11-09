"""Disposable real-Gateway fixture. No lifespan, provider calls, or production state."""
import base64
import hashlib
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
from unittest.mock import patch

GATEWAY = Path(os.environ.get('RELAY_GATEWAY_SOURCE', '/build/Developer/gateway-kh/repository')).resolve()
EXPECTED = os.environ.get('RELAY_GATEWAY_COMMIT', '5b0ed72')
head = subprocess.check_output(['git', '-C', str(GATEWAY), 'rev-parse', 'HEAD'], text=True).strip()
if not head.startswith(EXPECTED):
    raise RuntimeError('Gateway candidate changed; review it before updating the proof pin.')
workspace = Path(os.environ.get('RELAY_PROOF_ROOT', '/build/Developer/_orchestra/relay-lead/worker-contract-20261109'))
if not workspace.is_dir():
    raise RuntimeError('The configured isolated test workspace is unavailable.')
print('Creating isolated Gateway proof storage.', flush=True)
ROOT = Path(tempfile.mkdtemp(prefix='gateway-http-proof-', dir=workspace))
print('Loading reviewed Gateway source with isolated paths.', flush=True)
# These are the same isolated path boundaries as Gateway tests/_isolation.py.
for key in list(os.environ):
    if key.startswith('GATEWAY_'):
        del os.environ[key]
os.environ.update({
    'GATEWAY_APP_SUPPORT': str(ROOT), 'GATEWAY_DATA': str(ROOT / 'data'),
    'GATEWAY_ARTIFACTS': str(ROOT / 'data/artifacts'), 'GATEWAY_DB': str(ROOT / 'data/jobs.sqlite3'),
    'GATEWAY_CREDENTIALS': str(ROOT / 'data/credentials.json'),
    'GATEWAY_MODEL_CACHE': str(ROOT / 'data/models_cache.json'), 'GATEWAY_ENV_FILE': str(ROOT / '.env'),
    'GATEWAY_FLEET': str(ROOT / 'fleet.json'), 'GATEWAY_VOICES': str(ROOT / 'voices.json'),
    'GATEWAY_VOICES_OUT': str(ROOT / 'voices.json'), 'STUDIOHUB_READY_FILE': str(ROOT / 'no-hub.json'),
    'GATEWAY_NO_MIGRATE': '1', 'GATEWAY_HOST': '127.0.0.1', 'GATEWAY_API_KEY': '',
    'GATEWAY_EXTENSION_WORKERS': '1', 'GATEWAY_EXTENSION_WORKER_LEASE_SECONDS': '2',
    'GATEWAY_EXTENSION_WORKER_CLAIM_WAIT_SECONDS': '2',
    **{f'GATEWAY_BACKUP_{name.upper()}': str(ROOT / f'backup-{name}') for name in ['icloud', 'local', 'volumes', 'trash']},
})
sys.path.insert(0, str(GATEWAY))
from fastapi import HTTPException
from gateway import api, config, extension_workers as ew, runtime, video_capabilities
from gateway.providers import registry
# shortcut: uses the pinned Gateway test worker internals; review when that runtime changes.
from gateway.worker import Worker
from gateway.store import RECOVERY_UNKNOWN
import uvicorn

assert config.APP_SUPPORT.resolve() == ROOT
assert config.ARTIFACT_DIR.resolve().is_relative_to(ROOT)
assert runtime.store is not None
TEXT = next(r for r in video_capabilities.VIDEO_CAPABILITIES
            if r.provider_id == 'story-studio-video' and 'text-to-video' in r.modes)
definition = registry.definition('story-studio-video')
models = [{'model_id': r.model_id, 'modality': 'video', 'raw': {}}
          for r in video_capabilities.VIDEO_CAPABILITIES if r.provider_id == 'story-studio-video']
# Only this isolated process can select the extension adapter. No provider browser exists here.
patchers = [patch.object(registry.models, 'enabled_models', lambda modality: [(definition, model) for model in models] if modality == 'video' else []),
            patch.object(registry.credentials, 'is_enabled', lambda pid: pid == definition.id),
            patch.object(registry.credentials, 'resolved', lambda pid: {})]
for patcher in patchers:
    patcher.start()
offered = threading.Event()
real_offer = ew.broker.offer

def record_offer(*args, **kwargs):
    job = real_offer(*args, **kwargs)
    offered.set()
    return job

ew.broker.offer = record_offer
runs = {}

@api.app.get('/__relay_proof/info')
def info():
    return {'fixture': True, 'gateway_commit': head, 'root': str(ROOT),
            'model': TEXT.model_id, 'duration': TEXT.duration_min,
            'source_hashes': {name: hashlib.sha256((GATEWAY / name).read_bytes()).hexdigest()
                              for name in ['gateway/extension_workers.py', 'gateway/adapters/chrome_extension.py']}}

@api.app.post('/__relay_proof/offer')
def offer(mode: str = 'text'):
    if mode not in ('text', 'image'):
        raise HTTPException(422, 'Unsupported fixture mode.')
    if any(thread.is_alive() for thread in runs.values()):
        raise HTTPException(409, 'A fixture job is already running.')
    record = TEXT if mode == 'text' else next(r for r in video_capabilities.VIDEO_CAPABILITIES
        if r.provider_id == 'story-studio-video' and 'reference-to-video' in r.modes)
    params = {'prompt': 'Relay nonpaid HTTP fixture', 'duration_seconds': record.duration_min}
    if mode == 'image':
        config.ARTIFACT_DIR.mkdir(parents=True, exist_ok=True)
        reference = config.ARTIFACT_DIR / 'relay-reference.png'
        reference.write_bytes(base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+kfukAAAAASUVORK5CYII='))
        params.update(_reference_paths=[str(reference)], _reference_types=['image/png'])
    offered.clear()
    row, _ = runtime.store.submit(modality='video', model_id=record.model_id,
                                  params=params, idempotency_key=None, provider_id=definition.id)
    worker = Worker(runtime.store, interval_s=10.0, pool_size=0)
    thread = threading.Thread(target=worker._run, args=(runtime.store.get(row['id']),), daemon=True)
    runs[row['id']] = thread
    thread.start()
    if not offered.wait(5):
        raise HTTPException(500, 'The real adapter did not offer its fixture job.')
    return {'row_id': row['id']}

@api.app.get('/__relay_proof/settled/{row_id}')
def settled(row_id: str):
    thread = runs.get(row_id)
    if thread is None:
        raise HTTPException(404, 'Unknown fixture job.')
    thread.join(10)
    if thread.is_alive():
        raise HTTPException(409, 'The fixture worker has not settled.')
    row = runtime.store.get(row_id)
    artifacts = [{'name': str(p.relative_to(config.ARTIFACT_DIR)), 'bytes': p.stat().st_size,
                  'sha256': hashlib.sha256(p.read_bytes()).hexdigest()}
                 for p in config.ARTIFACT_DIR.rglob('*') if p.is_file()]
    return {'state': row['state'], 'error_code': row.get('error_code'),
            'recovery_state': row.get('recovery_state'), 'upstream_job_id': row.get('upstream_job_id'),
            'unknown': row.get('recovery_state') == RECOVERY_UNKNOWN, 'artifacts': artifacts}

if __name__ == '__main__':
    print(f'Relay isolated Gateway {head[:7]} on loopback7916; data retained at {ROOT}', flush=True)
    uvicorn.run(api.app, host='127.0.0.1', port=7916, lifespan='off', access_log=False, log_level='warning')

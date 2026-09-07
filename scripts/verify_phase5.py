"""Synthetic process crash/restart verification for persistent organization jobs.
Build memivy-core's memory_probe example first. No model calls or user data.
"""
import json
from pathlib import Path
import select
import sqlite3
import subprocess
import tempfile
import uuid

PROBE = Path(__file__).resolve().parents[1] / 'target/debug/examples/memory_probe'
with tempfile.TemporaryDirectory() as tmp:
    data = Path(tmp) / 'data'
    def call(*args):
        return subprocess.run([PROBE, data, *args], capture_output=True, text=True, check=True, timeout=10).stdout
    capture = json.loads(call('capture', str(uuid.uuid4()), '强杀时仍需保留的合成原话'))
    process = subprocess.Popen([PROBE, data, 'hold-organization'], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    try:
        assert select.select([process.stdout], [], [], 10)[0], 'claim acknowledgement missing'
        attempt = process.stdout.readline().strip()
        assert attempt
    finally:
        process.kill()
        process.wait(timeout=5)
    assert not process.stderr.read()
    receipt = json.loads(call('finish-organization'))
    assert receipt['request_id'] == attempt
    call('diagnostics')
    db = sqlite3.connect(data / 'memivy.db')
    assert db.execute('SELECT count(*) FROM receipts WHERE request_id=?', [attempt]).fetchone()[0] == 1
    assert db.execute('SELECT count(*) FROM memory_versions').fetchone()[0] == 1
    assert db.execute('SELECT status FROM organization_jobs WHERE capture_id=?', [capture['id']]).fetchone()[0] == 'done'
    assert db.execute('SELECT text FROM captures WHERE id=?', [capture['id']]).fetchone()[0] == capture['text']
    replay = subprocess.run([PROBE, data, 'finish-organization'], capture_output=True, text=True, timeout=10)
    assert replay.returncode != 0
    assert db.execute('SELECT count(*) FROM memory_versions').fetchone()[0] == 1
    print('PASS: acknowledged processing job survives SIGKILL; same attempt applies once; raw preserved.')

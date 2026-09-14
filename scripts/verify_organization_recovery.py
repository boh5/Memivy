"""Synthetic process crash/restart verification for persistent organization jobs.
Build memivy-core's memory_probe example first. No model calls or user data.
"""
import json
import os
from pathlib import Path
import select
import sqlite3
import subprocess
import tempfile
import uuid

PROBE = Path(os.environ.get('MEMIVY_TEST_PROBE', Path(__file__).resolve().parents[1] / 'target/debug/examples/memory_probe'))
with tempfile.TemporaryDirectory() as tmp:
    data = Path(tmp) / 'data'
    def call(*args):
        return subprocess.run([PROBE, data, *args], capture_output=True, text=True, check=True, timeout=10).stdout
    raw_text = '强杀时仍需保留的合成原话'
    capture = json.loads(call('capture', str(uuid.uuid4()), raw_text))
    db = sqlite3.connect(data / 'memivy.db')
    initial_versions = db.execute('SELECT id FROM memory_versions').fetchall()
    assert initial_versions == [(capture['version_id'],)], 'capture must immediately create a memory version'
    initial_memory = db.execute('SELECT id FROM memories').fetchall()
    assert initial_memory == [(capture['memory_id'],)]
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
    assert db.execute('SELECT count(*) FROM receipts WHERE request_id=?', [attempt]).fetchone()[0] == 1
    assert db.execute('SELECT count(*) FROM memory_versions').fetchone()[0] == len(initial_versions) + 1
    assert db.execute('SELECT id FROM memories').fetchall() == initial_memory
    assert db.execute('SELECT id FROM memory_versions WHERE id=?', initial_versions[0]).fetchall() == initial_versions
    assert db.execute('SELECT status FROM organization_jobs WHERE capture_id=?', [capture['capture_id']]).fetchone()[0] == 'done'
    assert db.execute('SELECT text FROM captures WHERE id=?', [capture['capture_id']]).fetchone()[0] == raw_text
    replay = subprocess.run([PROBE, data, 'finish-organization'], capture_output=True, text=True, timeout=10)
    assert replay.returncode != 0
    assert db.execute('SELECT count(*) FROM memory_versions').fetchone()[0] == len(initial_versions) + 1
    assert db.execute('SELECT id FROM memories').fetchall() == initial_memory
    assert db.execute('SELECT id FROM memory_versions WHERE id=?', initial_versions[0]).fetchall() == initial_versions
    print('PASS: acknowledged processing job survives SIGKILL; same attempt applies once; raw preserved.')

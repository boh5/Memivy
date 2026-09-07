"""Deterministic SIGKILL at harness checkpoints; only fresh synthetic directories.

Migration checkpoint uses repository SQL in a real SQLite transaction, not an
instrumented MemoryStore::open. This proves rollback/reopen, not disk power loss.
"""
import json
import os
from pathlib import Path
import select
import sqlite3
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
PROBE = Path(os.environ.get('MEMIVY_TEST_PROBE', ROOT/'target/debug/examples/memory_probe'))


def kill_at_checkpoint(data, command):
    process = subprocess.Popen([PROBE, data, command], stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, text=True)
    try:
        assert select.select([process.stdout], [], [], 10)[0], 'missing checkpoint'
        checkpoint = process.stdout.readline().strip()
        assert checkpoint, 'process exited before checkpoint'
    finally:
        if process.poll() is None:
            process.kill()
        process.wait(timeout=5)
    assert process.returncode == -9
    assert not process.stderr.read()
    return checkpoint


def call(data, command):
    result = subprocess.run([PROBE, data, command], capture_output=True, text=True,
                            check=True, timeout=10)
    assert not result.stderr
    return result.stdout


def main():
    with tempfile.TemporaryDirectory(prefix='memivy-core-faults-') as tmp:
        root = Path(tmp)
        data = root/'migration'
        kill_at_checkpoint(data, 'hold-migration')
        with sqlite3.connect(data/'memivy.db') as db:
            assert db.execute('PRAGMA user_version').fetchone()[0] == 0
            assert db.execute("SELECT count(*) FROM sqlite_master WHERE name IN ('captures','conversations')").fetchone()[0] == 0
        assert json.loads(call(data, 'diagnostics'))['integrity'] == 'ok'
        data = root/'turn'
        turn = kill_at_checkpoint(data, 'hold-turn')
        call(data, 'recover-turn')
        with sqlite3.connect(data/'memivy.db') as db:
            assert db.execute("SELECT status FROM messages WHERE turn_id=? AND role='assistant'", [turn]).fetchone()[0] == 'interrupted'
            for table in ['captures', 'memories', 'memory_versions', 'receipts']:
                assert db.execute(f'SELECT count(*) FROM {table}').fetchone()[0] == 0
        assert json.loads(call(data, 'diagnostics'))['integrity'] == 'ok'
        print('PASS: SIGKILL during migration transaction rolls back; reopen migrates cleanly.')
        print('PASS: SIGKILL during pending answer recovers interrupted; no durable memory writes.')


if __name__ == '__main__':
    main()

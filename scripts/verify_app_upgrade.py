"""Check packaged schema-2 upgrade on a disposable GitHub macOS runner only.

The baseline archive must first pass scripts/verify-update.mjs. This exercises
bundle replacement and migration, not the interactive updater or OS permissions.
"""
import argparse
import json
import os
from pathlib import Path, PurePosixPath
import plistlib
import shutil
import sqlite3
import subprocess
import sys
import tarfile
import tempfile
import time
import uuid

from verify_app_startup import WINDOW_PROBE
from verify_mcp import Client


def info(app):
    with (app / 'Contents/Info.plist').open('rb') as stream:
        value = plistlib.load(stream)
    if value['CFBundleIdentifier'] != 'com.memivy.app':
        raise RuntimeError('Expected the production application identity.')
    return value


def snapshot(database):
    with sqlite3.connect(database.as_uri() + '?mode=ro', uri=True) as connection:
        assert connection.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
        assert not connection.execute('PRAGMA foreign_key_check').fetchall()
        tables = ('captures', 'capture_state', 'memories', 'memory_versions',
                  'version_captures', 'receipts', 'workspace_drafts')
        return {table: connection.execute(f'SELECT * FROM {table} ORDER BY 1').fetchall()
                for table in tables}


def check(app, previous_archive):
    if sys.platform != 'darwin' or os.environ.get('GITHUB_ACTIONS') != 'true':
        raise RuntimeError('Run packaged upgrade verification only on a disposable GitHub macOS runner.')
    if subprocess.run(['pgrep', '-x', 'memivy'], capture_output=True).returncode != 1:
        raise RuntimeError('Another application process may be running.')
    candidate = info(app)
    subprocess.run(['codesign', '--verify', '--deep', '--strict', str(app)], check=True)
    with tempfile.TemporaryDirectory(prefix='memivy-package-upgrade-') as temporary:
        root = Path(temporary).resolve()
        baseline = root / 'baseline'
        baseline.mkdir()
        with tarfile.open(previous_archive, 'r:gz') as archive:
            for member in archive.getmembers():
                path = PurePosixPath(member.name)
                if path.is_absolute() or '..' in path.parts or not path.parts or path.parts[0] != 'Memivy.app':
                    raise RuntimeError('Unexpected baseline archive path.')
            archive.extractall(baseline, filter='data')
        old_app = baseline / 'Memivy.app'
        assert info(old_app)['CFBundleShortVersionString'] == '0.1.6'
        assert candidate['CFBundleShortVersionString'] != '0.1.6'
        subprocess.run(['codesign', '--verify', '--deep', '--strict', str(old_app)], check=True)
        installed = root / 'Applications/Memivy.app'
        installed.parent.mkdir()
        shutil.copytree(old_app, installed, symlinks=True)
        library = root / 'library'
        library.mkdir()
        desktop = {'login_initialized': True, 'visible': False, 'source_app': 'Upgrade QA'}
        updates = {'automatic': False}
        (library / 'desktop.json').write_text(json.dumps(desktop))
        (library / 'updates.json').write_text(json.dumps(updates))
        source = root / 'window.swift'
        source.write_text(WINDOW_PROBE)
        window_probe = root / 'window-probe'
        subprocess.run(['xcrun', 'swiftc', '-module-cache-path', str(root / 'swift-cache'),
                        str(source), '-o', str(window_probe)], check=True, timeout=120)
        environment = {key: value for key, value in os.environ.items() if not key.startswith('MEMIVY_')}
        environment['MEMIVY_DATA_DIR'] = str(library)

        def launch(schema, name):
            with (root / f'{name}.log').open('w') as log:
                process = subprocess.Popen([str(installed / 'Contents/MacOS/memivy')],
                                           env=environment, stdout=log, stderr=log)
                try:
                    deadline = time.monotonic() + 45
                    stable = None
                    while time.monotonic() < deadline:
                        if process.poll() is not None:
                            raise RuntimeError(f'{name} exited before startup completed: '
                                               + (root / f'{name}.log').read_text())
                        database = library / 'memivy.db'
                        ready = False
                        if database.exists():
                            with sqlite3.connect(database.as_uri() + '?mode=ro', uri=True) as db:
                                ready = db.execute('PRAGMA user_version').fetchone()[0] == schema
                        visible = subprocess.run([str(window_probe), str(process.pid)], timeout=5).returncode == 0
                        if ready and visible:
                            stable = stable or time.monotonic()
                            if time.monotonic() - stable >= 5:
                                return
                        else:
                            stable = None
                        time.sleep(0.5)
                    raise RuntimeError(f'{name} did not show a stable window with the expected schema: '
                                       + (root / f'{name}.log').read_text())
                finally:
                    if process.poll() is None:
                        process.terminate()
                        try:
                            process.wait(timeout=10)
                        except subprocess.TimeoutExpired:
                            process.kill()
                            process.wait(timeout=5)

        launch(2, 'baseline')
        client = Client(old_app / 'Contents/MacOS/memivy-mcp', library)
        original = 'Synthetic upgrade evidence\n原文及草稿必须保留。\n'
        try:
            client.tool('memory_capture', {'request_id': str(uuid.uuid4()), 'text': original,
                                         'source_app': 'Upgrade QA'})
        finally:
            client.close()
        # Seed an explicit synthetic legacy draft fixture, never a personal library.
        draft = {'key': 'capture', 'request_id': str(uuid.uuid4()), 'title': '',
                 'body': 'Unsent synthetic draft\n未发送的草稿', 'expected_version': None, 'context': []}
        with sqlite3.connect(library / 'memivy.db') as db:
            db.execute('PRAGMA foreign_keys=ON')
            db.execute('INSERT INTO workspace_drafts(key,payload) VALUES(?,?)',
                       ('capture', json.dumps(draft, ensure_ascii=False)))
        before = snapshot(library / 'memivy.db')
        assert before['captures'][0][3] == original, 'Baseline capture must retain the exact input.'
        shutil.rmtree(installed)
        shutil.copytree(app, installed, symlinks=True)
        subprocess.run(['codesign', '--verify', '--deep', '--strict', str(installed)], check=True)
        launch(3, 'upgraded')
        assert snapshot(library / 'memivy.db') == before, 'Upgrade changed existing durable material.'
        backups = list((library / 'recovery').glob('before-migration-2-to-3-*'))
        assert len(backups) == 1, 'Expected a single pre-migration backup.'
        assert snapshot(backups[0]) == before, 'Migration backup lost existing material.'
        with sqlite3.connect(backups[0].as_uri() + '?mode=ro', uri=True) as db:
            assert db.execute('PRAGMA user_version').fetchone()[0] == 2
        assert json.loads((library / 'updates.json').read_text()) == updates
        current_desktop = json.loads((library / 'desktop.json').read_text())
        assert all(current_desktop[key] == value for key, value in desktop.items())
        launch(3, 'restarted')
        assert snapshot(library / 'memivy.db') == before, 'Restart changed existing durable material.'
        assert list((library / 'recovery').glob('before-migration-2-to-3-*')) == backups
        old = subprocess.run([str(old_app / 'Contents/MacOS/memivy-mcp')], env=environment,
                             input='', text=True, capture_output=True, timeout=10)
        assert old.returncode != 0, 'The older sidecar must reject schema 3.'
        assert not old.stdout and 'Cannot open the memory library' in old.stderr, 'Reject at library open, not a missing handshake.'
        assert snapshot(library / 'memivy.db') == before
        return {'status': 'passed', 'from': '0.1.6', 'to': candidate['CFBundleShortVersionString'],
                'schema': '2 -> 3', 'checks': ['same-path bundle replacement', 'visible main window',
                'originals, versions, receipts and draft fixture unchanged', 'settings preserved',
                'schema-2 recovery backup', 'restart', 'older sidecar rejects schema 3', 'integrity ok'],
                'limits': ['No interactive updater, Gatekeeper, microphone or physical input acceptance.']}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path, required=True)
    parser.add_argument('--previous-archive', type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(check(args.app.resolve(), args.previous_archive.resolve()), indent=2))

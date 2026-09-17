"""Check the packaged main application on a disposable GitHub macOS runner.

This checks process survival, library initialization and a visible main window.
It does not replace downloaded-app installation or interactive acceptance tests.
Never run this release-identity check in a personal macOS account.
"""
import argparse
import json
import os
from pathlib import Path
import plistlib
import sqlite3
import subprocess
import sys
import tempfile
import time

WINDOW_PROBE = r'''
import CoreGraphics
import Foundation
let pid = Int(CommandLine.arguments[1])!
let windows = CGWindowListCopyWindowInfo(.optionOnScreenOnly, kCGNullWindowID) as? [[String: Any]] ?? []
let visible = windows.contains { window in
    guard (window[kCGWindowOwnerPID as String] as? Int) == pid,
          (window[kCGWindowLayer as String] as? Int) == 0,
          let bounds = window[kCGWindowBounds as String] as? [String: Any],
          let width = bounds["Width"] as? Double,
          let height = bounds["Height"] as? Double else { return false }
    return width >= 700 && height >= 500
}
exit(visible ? 0 : 1)
'''


def check(app):
    if sys.platform != 'darwin' or os.environ.get('GITHUB_ACTIONS') != 'true':
        raise RuntimeError('Run packaged startup verification only on a disposable GitHub macOS runner.')
    with (app/'Contents/Info.plist').open('rb') as stream:
        info = plistlib.load(stream)
    if info['CFBundleIdentifier'] != 'com.memivy.app':
        raise RuntimeError('Expected the production application identity.')
    executable = app/'Contents/MacOS'/info['CFBundleExecutable']
    existing = subprocess.run(['pgrep', '-x', info['CFBundleExecutable']], capture_output=True)
    if existing.returncode != 1:
        raise RuntimeError('Cannot verify startup while another application process may be running.')
    with tempfile.TemporaryDirectory(prefix='memivy-package-startup-') as temporary:
        root = Path(temporary).resolve()
        library = root/'library'
        library.mkdir()
        # Avoid registering a login item for a disposable application path.
        (library/'desktop.json').write_text(json.dumps({'login_initialized': True, 'visible': False}))
        source = root/'window.swift'
        source.write_text(WINDOW_PROBE)
        window_probe = root/'window-probe'
        subprocess.run(['xcrun', 'swiftc', '-module-cache-path', str(root/'swift-cache'),
                        str(source), '-o', str(window_probe)], check=True, timeout=120)
        env = {key: value for key, value in os.environ.items() if not key.startswith('MEMIVY_')}
        env['MEMIVY_DATA_DIR'] = str(library)
        log_path = root/'startup.log'
        with log_path.open('w') as log:
            process = subprocess.Popen([str(executable)], env=env, stdout=log, stderr=log)
            try:
                deadline = time.monotonic() + 45
                ready_since = None
                while time.monotonic() < deadline:
                    if process.poll() is not None:
                        raise RuntimeError(f'Application exited during startup ({process.returncode}): '
                                           + log_path.read_text())
                    database = library/'memivy.db'
                    initialized = False
                    if database.exists():
                        try:
                            with sqlite3.connect(database.as_uri()+'?mode=ro', timeout=1) as connection:
                                initialized = (connection.execute('PRAGMA application_id').fetchone()[0] == 0x4D454D59
                                               and connection.execute('PRAGMA user_version').fetchone()[0] == 1)
                        except sqlite3.Error:
                            pass
                    visible = subprocess.run([str(window_probe), str(process.pid)], timeout=5).returncode == 0
                    if initialized and visible:
                        if ready_since is None:
                            ready_since = time.monotonic()
                        if time.monotonic() - ready_since >= 5:
                            return {'status': 'passed', 'version': info['CFBundleShortVersionString'],
                                    'checks': ['packaged main process', 'isolated schema-1 library',
                                               'visible main window stable for five seconds']}
                    else:
                        ready_since = None
                    time.sleep(0.5)
                raise RuntimeError('Application did not initialize its isolated library and show its main window: '
                                   + log_path.read_text())
            finally:
                if process.poll() is None:
                    process.terminate()
                    try:
                        process.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait(timeout=5)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path, required=True)
    print(json.dumps(check(parser.parse_args().app.resolve()), indent=2))

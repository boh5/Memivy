"""Run Section 5 assets in isolated data; model calls require --model-config.

Evidence is append-only by run directory. A completed offline run does not mark
manual visual checks or semantic review as passed. No credentials are copied.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, help='fresh output directory under research/')
    parser.add_argument('--model-config', type=Path, help='explicit private config outside repository; sends synthetic fixtures only')
    parser.add_argument('--models-only', action='store_true', help='build and run both model corpora, without repeating offline suite')
    args = parser.parse_args()
    if args.models_only and not args.model_config:
        parser.error('--models-only requires --model-config')
    config = args.model_config.resolve() if args.model_config else None
    if config:
        if config.is_relative_to(ROOT) or not config.is_file() or config.stat().st_mode & 0o777 != 0o600:
            parser.error('configuration must be an external file with mode 0600')
    out = (args.output or ROOT/'research/core-tests'/f'{datetime.now(timezone.utc):%Y%m%dT%H%M%SZ}-{uuid.uuid4().hex[:8]}').resolve()
    if not out.is_relative_to(ROOT/'research'):
        parser.error('output must be under ignored research/')
    out.parent.mkdir(parents=True, exist_ok=True)
    out.mkdir(exist_ok=False)
    env = os.environ.copy()
    # Never allow inherited data/config overrides to select the user's library.
    for key in ['MEMIVY_DATA_DIR', 'MEMIVY_MODEL_CONFIG', 'MEMIVY_PHASE1_DATA_DIR', 'MEMIVY_PHASE1_MODEL_CONFIG']:
        env.pop(key, None)
    env['PATH'] = str(Path.home()/'.cargo/bin') + os.pathsep + env.get('PATH', '')
    report = {'started_utc': datetime.now(timezone.utc).isoformat(), 'platform': platform.platform(),
              'machine': platform.machine(), 'stages': [], 'visual_native': 'not_run',
              'human_semantic_review': 'pending', 'model_evaluation': 'not_run', 'status': 'running'}
    paths = [ROOT/'Cargo.lock', ROOT/'package-lock.json']
    for directory in ['crates', 'migrations', 'src', 'src-tauri/src', 'tests', 'scripts']:
        paths.extend(p for p in (ROOT/directory).rglob('*') if p.is_file() and '__pycache__' not in p.parts)
    report['file_sha256'] = {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(set(paths))}
    report['git_head'] = subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True, capture_output=True, check=True).stdout.strip()

    def save():
        (out/'summary.json').write_text(json.dumps(report, ensure_ascii=False, indent=2)+'\n')

    def run(name, command, timeout=1200):
        start = time.monotonic()
        print(f'RUN {name}', flush=True)
        path = out/f'{name}.log'
        with path.open('w') as log:
            try:
                result = subprocess.run([str(a) for a in command], cwd=ROOT, env=env,
                                        stdout=log, stderr=subprocess.STDOUT, timeout=timeout)
                code = result.returncode
            except subprocess.TimeoutExpired:
                code = 124
        report['stages'].append({'name': name, 'exit_code': code, 'elapsed_seconds': round(time.monotonic()-start, 2), 'log': path.name})
        save()
        print(f'{"PASS" if code == 0 else "FAIL"} {name}', flush=True)
        return code == 0

    save()
    try:
        if not args.models_only:
            for name, cmd in [
                ('rust-format', ['cargo', 'fmt', '--all', '--', '--check']),
                ('rust-lint', ['cargo', 'clippy', '--workspace', '--all-targets', '--offline', '--', '-D', 'warnings']),
                ('rust-tests', ['cargo', 'test', '--workspace', '--all-targets', '--offline']),
                ('ui-tests', ['npm', 'run', 'test:ui']),
                ('frontend-build', ['npm', 'run', 'build']),
            ]:
                run(name, cmd)
        built = run('harness-build', ['cargo', 'build', '-p', 'memivy-core', '--examples', '-p', 'memivy-mcp', '--bins', '--offline', '--message-format=json'])
        if not built:
            raise RuntimeError('harness build failed; process and model checks were not run')
        artifacts = {}
        for line in (out/'harness-build.log').read_text().splitlines():
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                continue
            if row.get('reason') == 'compiler-artifact' and row.get('executable'):
                artifacts[row['target']['name']] = Path(row['executable'])
        for name in ['memory_probe', 'memivy-mcp', 'intelligence_probe', 'discussion_probe', 'visual_fixture']:
            assert name in artifacts and artifacts[name].is_file(), f'missing current build artifact: {name}'
        env['MEMIVY_TEST_PROBE'] = str(artifacts['memory_probe'])
        report['binary_sha256'] = {name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in artifacts.items()}
        if not args.models_only:
            for script in ['verify_phase2', 'verify_phase5', 'verify_core_faults']:
                run(script, [sys.executable, ROOT/f'scripts/{script}.py'])
            run('verify_phase6', [sys.executable, ROOT/'scripts/verify_phase6.py', '--binary', artifacts['memivy-mcp']])
            run('visual-seed', [artifacts['visual_fixture'], out/'visual-data'])
            contract = json.loads((ROOT/'tests/assets/visual_contract.json').read_text())
            (out/'visual-review.json').write_text(json.dumps([
                {'case_id': c['id'], 'result': 'not_run', 'screenshot_path': '', 'notes': '', 'reviewer': ''}
                for c in contract['cases']], ensure_ascii=False, indent=2)+'\n')
        if config:
            for name in ['intelligence_probe', 'discussion_probe']:
                run(name, [artifacts[name], config, out/name], timeout=3600)
            report['model_evaluation'] = 'completed' if all(s['exit_code'] == 0 for s in report['stages'] if s['name'] in ['intelligence_probe', 'discussion_probe']) else 'has_failures'
            # Only inspect the key for leak detection; never print or persist it.
            key = json.loads(config.read_text()).get('api_key')
            if key:
                leaks = [str(p.relative_to(out)) for p in out.rglob('*') if p.is_file() and key.encode() in p.read_bytes()]
                report['credential_scan'] = {'passed': not leaks, 'files_with_match': leaks}
                if leaks:
                    raise RuntimeError('credential scan failed; keep this run private')
    except Exception as error:
        report['error'] = type(error).__name__ + ': ' + str(error)
        report['status'] = 'failed'
    else:
        report['status'] = 'failed' if any(s['exit_code'] != 0 for s in report['stages']) else 'passed'
    finally:
        report['finished_utc'] = datetime.now(timezone.utc).isoformat()
        save()
    print(f"{report['status'].upper()}: {out}", flush=True)
    return 0 if report['status'] == 'passed' else 1


if __name__ == '__main__':
    sys.exit(main())

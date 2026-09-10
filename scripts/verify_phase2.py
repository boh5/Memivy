"""Real process writers, acknowledged SIGKILL recovery and live SQLite backup.

Run after cargo build -p memivy-core --example memory_probe --offline.
Uses only a temporary, isolated directory and synthetic input.
"""
import concurrent.futures
import json
import os
from pathlib import Path
import select
import sqlite3
import subprocess
import tempfile
import threading
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
PROBE = Path(os.environ.get("MEMIVY_TEST_PROBE", ROOT / "target/debug/examples/memory_probe"))


def check(root):
    data = root / "data"

    # Exercise first-open races too, before any process has migrated a database.
    for round_number in range(8):
        fresh = root / f"fresh-{round_number}"
        barrier = threading.Barrier(4)

        def first_open(_):
            barrier.wait(timeout=10)
            for _ in range(20):
                result = subprocess.run(
                    [PROBE, fresh, "diagnostics"], capture_output=True,
                    text=True, timeout=10,
                )
                if result.returncode == 75:
                    time.sleep(0.01)
                    continue
                result.check_returncode()
                assert not result.stderr
                assert json.loads(result.stdout)["integrity"] == "ok"
                return
            raise AssertionError("fresh database stayed busy")

        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
            list(pool.map(first_open, range(4)))

    def call(command, *args):
        output = subprocess.run(
            [PROBE, data, command, *args], capture_output=True, text=True,
            check=True, timeout=10,
        )
        assert not output.stderr, "normal execution should not log content"
        return json.loads(output.stdout) if output.stdout.strip() else None

    call("diagnostics")
    with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
        results = list(pool.map(
            lambda n: call("capture", str(uuid.uuid4()), f"独立进程原话 {n}"), range(24)
        ))
        assert len({r["memory_id"] for r in results}) == 24
        retry = str(uuid.uuid4())
        results = list(pool.map(lambda _: call("capture", retry, "同一确认重试"), range(8)))
        assert len({r["memory_id"] for r in results}) == 1

    for command in ("hold", "hold-memory"):
        process = subprocess.Popen(
            [PROBE, data, command, str(uuid.uuid4()), f"保存后强杀 {command}"],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        )
        try:
            assert select.select([process.stdout], [], [], 10)[0], "no commit acknowledgement"
            acknowledged = json.loads(process.stdout.readline())
        finally:
            process.kill()
            process.wait(timeout=5)
        assert process.returncode == -9
        with sqlite3.connect(data / "memivy.db") as db:
            if command == "hold":
                assert db.execute("SELECT text FROM captures WHERE id=?", (acknowledged["capture_id"],)).fetchone()[0] == f"保存后强杀 {command}"
            else:
                assert db.execute("SELECT current_version_id FROM memories WHERE id=?", (acknowledged["memory_id"],)).fetchone()[0] == acknowledged["after_version"]
                assert db.execute("SELECT count(*) FROM receipt_changes WHERE request_id=?", (acknowledged["request_id"],)).fetchone()[0] == 1

    # A live writer runs while the backup API copies a consistent database.
    backup = root / "live-backup.sqlite3"
    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
        writes = pool.submit(lambda: [call("capture", str(uuid.uuid4()), f"备份时写入 {n}") for n in range(20)])
        call("backup", str(backup))
        writes.result(timeout=30)
    with sqlite3.connect(backup) as db:
        assert db.execute("PRAGMA integrity_check").fetchone()[0] == "ok"
        assert not db.execute("PRAGMA foreign_key_check").fetchall()
        assert db.execute("SELECT count(*) FROM captures").fetchone()[0] >= 27
        assert db.execute("SELECT count(*) FROM memory_versions").fetchone()[0] == db.execute("SELECT count(*) FROM captures").fetchone()[0] + 1
        assert db.execute("SELECT count(*) FROM captures c LEFT JOIN capture_state s ON s.capture_id=c.id WHERE s.capture_id IS NULL").fetchone()[0] == 0

    restored = root / "restored"
    subprocess.run([PROBE, restored, "restore", backup], capture_output=True,
                   text=True, check=True, timeout=10)
    with sqlite3.connect(restored / "memivy.db") as db:
        assert db.execute("PRAGMA integrity_check").fetchone()[0] == "ok"
        assert db.execute("SELECT count(*) FROM captures").fetchone()[0] >= 27
        assert db.execute("SELECT count(*) FROM memory_versions").fetchone()[0] == db.execute("SELECT count(*) FROM captures").fetchone()[0] + 1

    result = call("diagnostics")
    assert result["captures"] == 47, result
    assert result["versions"] == 48, result
    return {"status": "passed", **result, "checks": [
        "8 fresh databases opened by 4 concurrent processes each",
        "24 separate process writers", "8 process retries create one capture",
        "acknowledged raw capture survives SIGKILL", "version and receipt survive SIGKILL",
        "consistent online backup during concurrent writes and fresh-directory restore", "foreign keys and integrity",
    ]}


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="memivy-phase2-") as temporary:
        print(json.dumps(check(Path(temporary)), ensure_ascii=False, indent=2))

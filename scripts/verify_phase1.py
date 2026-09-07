"""Phase 1 process-level acceptance. Run after building MCP and the probe example."""
import concurrent.futures
import json
import os
from pathlib import Path
import select
import sqlite3
import subprocess
import tempfile
import uuid

ROOT = Path(__file__).resolve().parents[1]
PROBE = ROOT / "target/debug/examples/probe"
MCP = ROOT / "target/debug/memivy-mcp-prototype"


def check(data_dir):
    env = {**os.environ, "MEMIVY_PHASE1_DATA_DIR": str(data_dir)}

    def probe(*args):
        return subprocess.run([PROBE, *args], env=env, text=True, capture_output=True, check=True).stdout

    probe("diagnostics")
    with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
        results = list(pool.map(lambda n: probe("capture", f"跨进程原话 {n}"), range(24)))
    assert len({json.loads(value)["id"] for value in results}) == 24
    held = subprocess.Popen([PROBE, "hold", "确认保存后强制终止"], env=env, stdout=subprocess.PIPE, text=True)
    assert select.select([held.stdout], [], [], 5)[0], "capture acknowledgement timeout"
    acknowledged = json.loads(held.stdout.readline())
    held.kill()
    held.wait(timeout=5)
    assert json.loads(probe("search", "强制终止"))["items"][0]["id"] == acknowledged["id"]

    server = subprocess.Popen([MCP], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    serial = 0

    def call(method, params=None):
        nonlocal serial
        serial += 1
        request = {"jsonrpc": "2.0", "id": serial, "method": method}
        if params is not None:
            request["params"] = params
        server.stdin.write(json.dumps(request, ensure_ascii=False) + "\n")
        server.stdin.flush()
        assert select.select([server.stdout], [], [], 8)[0], f"MCP {method} timed out"
        value = json.loads(server.stdout.readline())
        assert value["id"] == serial, value
        assert "error" not in value, value
        return value["result"]

    try:
        initialized = call("initialize", {"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"memivy-process-test","version":"0.1"}})
        server.stdin.write('{"jsonrpc":"2.0","method":"notifications/initialized"}\n')
        server.stdin.flush()
        tools = call("tools/list")["tools"]
        assert [tool["name"] for tool in tools] == ["memory_capture"]
        request = {"name":"memory_capture","arguments":{"request_id":str(uuid.uuid4()),"text":"明确要求保存的 MCP 原话","source_app":"Process test"}}
        assert call("tools/call", request)["isError"] is True
        probe("mcp-on")
        result = call("tools/call", request)
        assert not result.get("isError", False), result
        receipt = json.loads(result["content"][0]["text"])
        again = call("tools/call", request)
        assert json.loads(again["content"][0]["text"])["capture_id"] == receipt["capture_id"]
        assert json.loads(probe("search", "MCP 原话"))["items"][0]["id"] == receipt["capture_id"]
        probe("mcp-off")
        request["arguments"]["request_id"] = str(uuid.uuid4())
        assert call("tools/call", request)["isError"] is True
        data_dir.joinpath("mcp.json").write_text("corrupt")
        assert call("tools/call", request)["isError"] is True
        server.stdin.close()
        assert server.wait(timeout=5) == 0
        assert server.stdout.read() == "", "stdout must contain only expected protocol responses"
        assert server.stderr.read() == "", "normal shutdown must not log captured content"
    finally:
        if server.poll() is None:
            server.kill()
            server.wait(timeout=5)

    diag = json.loads(probe("diagnostics"))
    assert diag["count"] == 26, diag
    db = sqlite3.connect(data_dir / "phase1.sqlite3")
    assert db.execute("PRAGMA integrity_check").fetchone()[0] == "ok"
    db.close()
    return {"status":"passed","records":diag["count"],"sqlite_version":diag["sqlite_version"],"mcp_protocol":initialized["protocolVersion"],"checks":["24 concurrent process writers","acknowledged capture survives SIGKILL","real stdio initialize/list/call","default off and live switch changes","retry deduplication","shared core retrieval","EOF clean exit","database integrity"]}


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="memivy-phase1-") as temporary:
        print(json.dumps(check(Path(temporary)), ensure_ascii=False, indent=2))

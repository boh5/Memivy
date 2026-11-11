"""MCP stdio and multi-process checks, exclusively in temporary data.

Build memivy-mcp and memivy-core's memory_probe first. An optional --binary
points at the actual bundled sidecar. No model or client credentials required.
"""
import argparse
import concurrent.futures
import fcntl
import json
import os
from pathlib import Path
import queue
import sqlite3
import subprocess
import tempfile
import threading
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
META = {"io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": {"name": "memivy-synthetic-check", "version": "1"},
        "io.modelcontextprotocol/clientCapabilities": {}}

class Client:
    def __init__(self, binary, data, latest=False, finish_handshake=True):
        self.latest, self.serial = latest, 0
        self.process = subprocess.Popen([str(binary)], env={**os.environ, "MEMIVY_DATA_DIR": str(data)},
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        self.lines = queue.Queue()
        def read():
            for line in self.process.stdout:
                self.lines.put(line)
            self.lines.put(None)
        threading.Thread(target=read, daemon=True).start()
        if latest:
            discovery = self.call("server/discover")
            assert discovery["supportedVersions"] == ["2025-11-25", "2026-07-28"], discovery
        else:
            info = self.call("initialize", {"protocolVersion":"2025-11-25", "capabilities":{},
                                            "clientInfo":{"name":"memivy-synthetic-check","version":"1"}})
            assert info["serverInfo"]["name"] == "memivy"
            assert info["protocolVersion"] == "2025-11-25"
            if finish_handshake:
                self.process.stdin.write('{"jsonrpc":"2.0","method":"notifications/initialized"}\n')
                self.process.stdin.flush()
    def call(self, method, params=None, error=False):
        self.serial += 1
        params = dict(params or {})
        if self.latest: params["_meta"] = META
        self.process.stdin.write(json.dumps({"jsonrpc":"2.0","id":self.serial,"method":method,"params":params})+'\n')
        self.process.stdin.flush()
        while True:
            line = self.lines.get(timeout=10)
            assert line, "MCP exited before response"
            reply = json.loads(line)
            if "id" not in reply: continue
            assert reply["id"] == self.serial, reply
            assert ("error" in reply) == error, reply
            return reply.get("error" if error else "result")
    def tool(self, name, args, failed=False):
        result = self.call("tools/call", {"name": name, "arguments": args})
        assert bool(result.get("isError")) == failed, result
        assert json.loads(result["content"][0]["text"]) == result["structuredContent"]
        return result["structuredContent"]
    def close(self, kill=False):
        if kill: self.process.kill()
        else: self.process.stdin.close()
        self.process.wait(timeout=8)
        if not kill: assert self.process.returncode == 0
        stderr = self.process.stderr.read()
        assert not stderr, stderr

def check_update_shutdown(binary, data, connected, args, receipt):
    preference = (data/'mcp.json').read_bytes()
    initializing = Client(binary, data, finish_handshake=False)
    sessions = (connected, initializing)
    in_flight = {**args, 'request_id': str(uuid.uuid4()), 'text': 'Capture in progress during update',
                 'title': 'In-flight capture', 'body': 'This capture started before the update.'}
    try:
        with concurrent.futures.ThreadPoolExecutor(max_workers=1) as worker, \
                sqlite3.connect(data/'memivy.db') as db, \
                (data/'mcp-admission.lock').open('r+b') as admission, \
                (data/'mcp.lock').open('r+b') as requests, \
                (data/'mcp-update.lock').open('r+b') as update:
            db.execute('BEGIN IMMEDIATE')
            reply = worker.submit(connected.tool, 'memory_capture', in_flight)
            deadline = time.monotonic() + 4
            while True:
                try:
                    fcntl.flock(requests, fcntl.LOCK_EX | fcntl.LOCK_NB)
                except BlockingIOError:
                    break
                fcntl.flock(requests, fcntl.LOCK_UN)
                assert time.monotonic() < deadline, "MCP capture did not enter the request gate"
                time.sleep(.01)
            # Close admission and drain the actual in-flight request before notifying sessions.
            fcntl.flock(admission, fcntl.LOCK_EX)
            db.rollback()
            fcntl.flock(requests, fcntl.LOCK_EX)
            fcntl.flock(update, fcntl.LOCK_EX)
            updated_receipt = reply.result(timeout=6)
            for client in sessions:
                assert client.process.wait(timeout=6) == 0
                assert not client.process.stdin.closed, "Update must close without client EOF"
                client.close()
            raced = subprocess.run([str(binary)], env={**os.environ, "MEMIVY_DATA_DIR": str(data)},
                                   input='', capture_output=True, text=True, timeout=4)
            assert raced.returncode == 1 and not raced.stdout
            assert raced.stderr.strip() == 'Cannot open the memory library', raced.stderr
    finally:
        for client in sessions:
            if client.process.poll() is None:
                client.close(kill=True)
    assert (data/'mcp.json').read_bytes() == preference
    reconnected = Client(binary, data, latest=True)
    try:
        assert reconnected.tool('memory_capture', args)['capture_id'] == receipt['capture_id']
        assert reconnected.tool('memory_capture', in_flight)['capture_id'] == updated_receipt['capture_id']
    finally:
        reconnected.close()

def check(binary):
    with tempfile.TemporaryDirectory(prefix="memivy-mcp-") as tmp:
        data = Path(tmp)/"library"
        client = Client(binary, data)
        def switch(enabled):
            temp = data/"mcp-test.tmp"
            temp.write_text(json.dumps({"enabled":enabled}))
            temp.chmod(0o600)
            temp.replace(data/"mcp.json")
        args = {
            "request_id": str(uuid.uuid4()),
            "text": "I set the field kit budget to 750.\nRun the temporary receipt check.",
            "title": "Field kit budget",
            "body": "The user set the field kit budget to 750. This is synthetic evidence.",
            "source_app": "Synthetic Agent",
            "project": "Test project",
            "session_uri": "https://example.test/session",
        }
        tools = {tool["name"]: tool for tool in client.call("tools/list")["tools"]}
        assert sorted(tools) == ["memory_capture", "memory_search"]
        assert set(tools["memory_capture"]["inputSchema"]["required"]) == {"request_id", "text", "title", "body", "source_app"}
        switch(False)
        for name, payload in [("memory_capture",args),("memory_search",{"queries":[{"text":"synthetic evidence","keywords":["synthetic", "evidence"]}]})]:
            assert client.tool(name,payload,failed=True)["code"] == "mcp_disabled"
        switch(True)
        missing_content = {key: value for key, value in args.items() if key not in {"title", "body"}}
        missing_content["request_id"] = str(uuid.uuid4())
        assert client.call("tools/call", {"name": "memory_capture", "arguments": missing_content})["isError"]
        receipt = client.tool("memory_capture",args)
        assert receipt["action"] == "memory_saved"
        assert client.tool("memory_capture",args)["capture_id"] == receipt["capture_id"]
        for field in ("text", "title", "body"):
            assert client.tool("memory_capture", {**args, field: "Conflicting retry"}, failed=True)["code"] == "request_conflict"
        hit = client.tool("memory_search",{"queries":[{"text":"synthetic evidence","keywords":["synthetic", "evidence"]}]})["items"][0]
        assert hit["record"]["id"] == receipt["memory_id"]
        assert hit["title"] == args["title"] and hit["snippet"] == args["body"]
        assert hit["origin"]["project"] == "Test project"
        assert not client.tool("memory_search", {"queries":[{"text":"temporary receipt check","keywords":["temporary receipt check"]}]})["items"]
        # Both permitted fields can expand to six bytes per character on the wire.
        escaped = {
            **args, "request_id": str(uuid.uuid4()), "title": "Escaped field limits",
            "text": "Original " + "\x0b" * (128 * 1024 - len("Original ")),
            "body": "Organized " + "\x0b" * (128 * 1024 - len("Organized ")),
        }
        assert len(json.dumps(escaped).encode()) > 1024 * 1024
        assert client.tool("memory_capture", escaped)["action"] == "memory_saved"
        assert client.call("tools/call", {"name":"memory_search","arguments":{"queries":[{"text":"synthetic evidence","keywords":["synthetic", "evidence"]}],"trash":True}})["isError"]
        client.call("tools/call", {"name":"memory_delete","arguments":{}},error=True)
        for payload in [{"queries":[{"text":"","keywords":[]}]},{"queries":[{"text":"synthetic","keywords":["synthetic"]}],"limit":9},{"queries":[{"text":"synthetic","keywords":["synthetic"]}],"limit":0}]:
            assert client.tool("memory_search",payload,failed=True)["code"] == "invalid_input"
        # Acknowledged transaction survives abrupt MCP process death.
        client.close(kill=True)
        client = Client(binary,data,latest=True)
        assert client.tool("memory_search",{"queries":[{"text":"synthetic evidence","keywords":["synthetic", "evidence"]}]})["items"][0]["record"]["id"] == receipt["memory_id"]
        switch(False)
        assert client.tool("memory_search",{"queries":[{"text":"synthetic evidence","keywords":["synthetic", "evidence"]}]},failed=True)["code"] == "mcp_disabled"
        switch(True)
        # Independent MCP processes and native core write concurrently. Identical
        # retries across processes must still produce a single durable capture.
        shared = {**args, "request_id": str(uuid.uuid4()), "text": "Concurrent idempotent synthetic evidence",
                  "title": "Concurrent capture", "body": "A single memory survives concurrent retries."}
        def writer(n):
            if n % 3 == 0:
                probe = Path(os.environ.get('MEMIVY_TEST_PROBE', ROOT/'target/debug/examples/memory_probe'))
                p = subprocess.run([str(probe),str(data),'capture',str(uuid.uuid4()),f'Concurrent original text from the app {n}'],capture_output=True,text=True,timeout=12)
                assert p.returncode == 0, p.stderr
                return None
            c = Client(binary,data,latest=bool(n%2))
            saved = c.tool('memory_capture',shared);c.close();return saved['capture_id']
        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
            ids = list(pool.map(writer,range(12)))
        assert len(set(i for i in ids if i)) == 1
        db = sqlite3.connect(data/'memivy.db')
        assert db.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
        assert db.execute('SELECT COUNT(*) FROM captures WHERE request_id=?',[shared['request_id']]).fetchone()[0] == 1
        assert db.execute('SELECT text FROM captures WHERE request_id=?',[args['request_id']]).fetchone()[0] == args['text']
        assert db.execute('SELECT count(*) FROM captures WHERE request_id=?', [missing_content['request_id']]).fetchone()[0] == 0
        check_update_shutdown(binary, data, client, args, receipt)
        # Protocol metadata works while disabled, but no data is returned.
        switch(False)
        client = Client(binary,data,latest=True)
        assert len(client.call('tools/list')['tools']) == 2
        assert client.tool('memory_capture',args,failed=True)['code'] == 'mcp_disabled'
        client.close()
        # Oversized frames terminate without content appearing on stderr/stdout.
        p = subprocess.run([str(binary)],env={**os.environ,'MEMIVY_DATA_DIR':str(data)},input='x'*(2*1024*1024+2)+'\n',capture_output=True,text=True,timeout=10)
        assert 'xxxxx' not in p.stderr and not p.stdout
        assert 'Invalid or oversized MCP request frame' in p.stderr
        assert len(p.stderr) < 200
    return {"status":"passed","protocols":["2025-11-25","2026-07-28"],"checks":["real stdio discovery/list/call", "explicit disable and live disable", "searchable organized content and separate verbatim source", "retry and conflict", "bounded arguments and tool surface", "12 concurrent native/MCP writers", "SIGKILL durability", "isolated data", "bounded input frames", "clean EOF", "update closes live and initializing stdio sessions while preserving settings and data", "SQLite integrity"]}

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary',type=Path,default=ROOT/'target/debug/memivy-mcp')
    print(json.dumps(check(parser.parse_args().binary.resolve()),ensure_ascii=False,indent=2))

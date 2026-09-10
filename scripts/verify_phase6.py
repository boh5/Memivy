"""Real stdio / multi-process Phase 6 checks, exclusively in temporary data.

Build memivy-mcp and memivy-core's memory_probe first. An optional --binary
points at the actual bundled sidecar. No model or client credentials required.
"""
import argparse
import concurrent.futures
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
    def __init__(self, binary, data, latest=False):
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

def check(binary):
    with tempfile.TemporaryDirectory(prefix="memivy-phase6-") as tmp:
        data = Path(tmp)/"formal"
        client = Client(binary, data)
        def switch(enabled):
            temp = data/"mcp-test.tmp"
            temp.write_text(json.dumps({"enabled":enabled}))
            temp.chmod(0o600)
            temp.replace(data/"mcp.json")
        args = {"request_id":str(uuid.uuid4()),"text":"明确授权保存 MCP 合成证据\n原话不能改写。", "source_app":"Synthetic Agent","project":"测试项目","session_uri":"https://example.test/session"}
        names = sorted(t["name"] for t in client.call("tools/list")["tools"])
        assert names == ["memory_capture", "memory_search"]
        for name, payload in [("memory_capture",args),("memory_search",{"query":"合成证据"})]:
            assert client.tool(name,payload,failed=True)["code"] == "mcp_disabled"
        switch(True)
        receipt = client.tool("memory_capture",args)
        assert client.tool("memory_capture",args)["capture_id"] == receipt["capture_id"]
        assert client.tool("memory_capture",{**args,"text":"不一致重试"},failed=True)["code"] == "request_conflict"
        hit = client.tool("memory_search",{"query":"合成证据"})["items"][0]
        assert hit["record"]["id"] == receipt["memory_id"]
        assert hit["origin"]["project"] == "测试项目"
        assert client.call("tools/call", {"name":"memory_search","arguments":{"query":"合成证据","trash":True}})["isError"]
        client.call("tools/call", {"name":"memory_delete","arguments":{}},error=True)
        for payload in [{"query":""},{"query":"合成","limit":9},{"query":"合成","limit":0}]:
            assert client.tool("memory_search",payload,failed=True)["code"] == "invalid_input"
        # Acknowledged transaction survives abrupt MCP process death.
        client.close(kill=True)
        client = Client(binary,data,latest=True)
        assert client.tool("memory_search",{"query":"合成证据"})["items"][0]["record"]["id"] == receipt["memory_id"]
        switch(False)
        assert client.tool("memory_search",{"query":"合成证据"},failed=True)["code"] == "mcp_disabled"
        switch(True)
        # Independent MCP processes and native core write concurrently. Identical
        # retries across processes must still produce a single durable capture.
        shared = {**args,"request_id":str(uuid.uuid4()),"text":"并发幂等合成证据"}
        def writer(n):
            if n % 3 == 0:
                probe = Path(os.environ.get('MEMIVY_TEST_PROBE', ROOT/'target/debug/examples/memory_probe'))
                p = subprocess.run([str(probe),str(data),'capture',str(uuid.uuid4()),f'应用侧并发原话 {n}'],capture_output=True,text=True,timeout=12)
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
        assert db.execute("SELECT count(*) FROM organization_jobs WHERE status='pending'").fetchone()[0] == 6
        assert not (data/'phase1.sqlite3').exists()
        client.close()
        # Protocol metadata works while disabled, but no data is returned.
        switch(False)
        client = Client(binary,data,latest=True)
        assert len(client.call('tools/list')['tools']) == 2
        assert client.tool('memory_capture',args,failed=True)['code'] == 'mcp_disabled'
        client.close()
        # Oversized frames terminate without content appearing on stderr/stdout.
        p = subprocess.run([str(binary)],env={**os.environ,'MEMIVY_DATA_DIR':str(data)},input='x'*(1024*1024+2)+'\n',capture_output=True,text=True,timeout=10)
        assert 'xxxxx' not in p.stderr and not p.stdout
        assert len(p.stderr) < 200
    return {"status":"passed","protocols":["2025-11-25","2026-07-28"],"checks":["real stdio discovery/list/call", "default off and live disable", "exact text and provenance", "retry and conflict", "bounded arguments and tool surface", "12 concurrent native/MCP writers", "SIGKILL durability", "pending organization without app", "isolated data", "bounded input frames", "clean EOF", "SQLite integrity"]}

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary',type=Path,default=ROOT/'target/debug/memivy-mcp')
    print(json.dumps(check(parser.parse_args().binary.resolve()),ensure_ascii=False,indent=2))

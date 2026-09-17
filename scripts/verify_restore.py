"""Whole-library restore with a live stdio MCP process; synthetic temporary data only."""
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import threading
import uuid
from verify_mcp import Client, ROOT


def check():
    binary = ROOT / 'target/debug/memivy-mcp'
    probe = ROOT / 'target/debug/examples/memory_probe'
    with tempfile.TemporaryDirectory(prefix='memivy-restore-process-') as tmp:
        root = Path(tmp) / 'library'
        def run(*args):
            result = subprocess.run([str(probe), str(root), *map(str, args)],
                                    capture_output=True, text=True, timeout=15)
            if result.returncode:
                raise RuntimeError(f'Probe {args[0]} failed ({result.returncode}): {result.stderr.strip()}')
            return result.stdout
        run('capture',str(uuid.uuid4()),'Synthetic record at backup')
        (root/'mcp.json').write_text('{"enabled":true}')
        snapshot=Path(tmp)/'chosen.db';run('backup',snapshot)
        prepared=json.loads(run('prepare-restore',snapshot))
        client=Client(binary,root)
        saved=[];first=threading.Event();stop=threading.Event();errors=[]
        def writer():
            try:
                for i in range(100):
                    if stop.is_set():break
                    result=client.call('tools/call',{'name':'memory_capture','arguments':{'request_id':str(uuid.uuid4()),'text':f'Subsequent synthetic record {i}','source_app':'Restore QA'}})
                    if result.get('isError'):break
                    saved.append(result['structuredContent']['capture_id']);first.set()
            except BaseException as error:
                errors.append(error);first.set()
        thread=threading.Thread(target=writer)
        thread.start()
        try:
            assert first.wait(5)
            run('arm-restore',prepared['id']);stop.set();thread.join(10);assert not thread.is_alive();assert not errors,errors
            assert saved
            # A newly launched MCP must fail before initializing or writing this database.
            newcomer=subprocess.run([str(binary)],env={**os.environ,'MEMIVY_DATA_DIR':str(root)},input='',capture_output=True,text=True,timeout=5)
            assert newcomer.returncode!=0 and newcomer.stdout=='',newcomer.stdout
            blocked=client.call('tools/call',{'name':'memory_capture','arguments':{'request_id':str(uuid.uuid4()),'text':'Must not be written','source_app':'Restore QA'}})
            assert blocked.get('isError'),blocked
            outcome=json.loads(run('open-application'));assert outcome['restored'],outcome
            before=sqlite3.connect(f"file:{outcome['previous_backup']}?mode=ro",uri=True)
            current=sqlite3.connect(f'file:{root / "memivy.db"}?mode=ro',uri=True)
            try:
                assert current.execute('SELECT count(*) FROM captures').fetchone()[0]==1
                retained={r[0] for r in before.execute('SELECT id FROM captures')}
                assert set(saved)<=retained
                assert before.execute('PRAGMA integrity_check').fetchone()[0]=='ok'
                assert current.execute('PRAGMA integrity_check').fetchone()[0]=='ok'
            finally: before.close();current.close()
            # The already-connected stdio server follows the stable root after restore.
            result=client.tool('memory_search',{'query':'backup'})
            assert result['items'],result
            client.close()
        finally:
            stop.set()
            if client.process.poll() is None:
                client.process.kill()
                client.process.wait(timeout=8)
            thread.join(10)
            assert not thread.is_alive(), 'MCP writer did not stop'
        print(json.dumps({'live_mcp_writes_preserved':len(saved),'new_mcp_startup_blocked':True,'existing_mcp_blocked_during_restore':True,'existing_mcp_reuses_restored_library':True,'integrity':'ok'},ensure_ascii=False))

if __name__=='__main__':check()

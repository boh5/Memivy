"""Check data isolation before the native launcher can touch any library."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
import os
import signal
import sys
import subprocess
import time
import unicodedata

spec = importlib.util.spec_from_file_location("dev_runtime", Path(__file__).with_name("dev-runtime.py"))
runtime = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runtime)


class DataIsolationTests(unittest.TestCase):
    def test_defaults_and_explicit_qa_library(self):
        with tempfile.TemporaryDirectory() as folder:
            home = Path(folder).resolve()
            self.assertEqual(runtime.data_root(home, None),
                             home / "Library/Application Support/com.memivy.app.dev")
            isolated = home / "qa"
            self.assertEqual(runtime.data_root(home, str(isolated), qa=True), isolated)
            with self.assertRaises(ValueError):
                runtime.data_root(home, None, qa=True)

    def test_protected_paths_and_symlinks_are_rejected(self):
        with tempfile.TemporaryDirectory() as folder:
            home = Path(folder).resolve()
            production = home / "Library/Application Support/com.memivy.app"
            production.mkdir(parents=True)
            alias = home / "alias"
            alias.symlink_to(production, target_is_directory=True)
            for path in [production, alias, alias / "new", home, Path("relative"), home / "x/../y"]:
                with self.subTest(path=path), self.assertRaises(ValueError):
                    runtime.data_root(home, str(path))
            development = home / "Library/Application Support/com.memivy.app.dev"
            with self.assertRaises(ValueError):
                runtime.data_root(home, str(development), qa=True)

    def test_case_aliases_cannot_select_existing_or_future_protected_libraries(self):
        with tempfile.TemporaryDirectory() as folder:
            home = Path(folder).resolve()
            support = home / "Library/Application Support"
            for existing in [False, True]:
                for identifier in ["com.memivy.app", "com.memivy.app.dev"]:
                    protected = support / identifier
                    if existing:
                        protected.mkdir(parents=True, exist_ok=True)
                    alias = support / identifier.upper()
                    for path in [alias, alias / "qa", home / "LIBRARY"]:
                        with self.subTest(existing=existing, path=path), self.assertRaises(ValueError):
                            runtime.data_root(home, str(path), qa=True)

    def test_unicode_alias_cannot_select_a_relocated_development_library(self):
        with tempfile.TemporaryDirectory() as folder:
            home = Path(folder).resolve()
            relocated = home / "café"
            relocated.mkdir()
            development = home / "Library/Application Support/com.memivy.app.dev"
            development.parent.mkdir(parents=True)
            development.symlink_to(relocated, target_is_directory=True)
            alias = unicodedata.normalize("NFD", str(relocated))
            with self.assertRaises(ValueError):
                runtime.data_root(home, alias, qa=True)


class ProcessOwnershipTests(unittest.TestCase):
    def test_cancellation_bounds_an_uncooperative_direct_child(self):
        self.check_cancellation(signal.SIGTERM)

    def test_terminal_hangup_stops_the_detached_child(self):
        self.check_cancellation(signal.SIGHUP)

    def check_cancellation(self, termination):
        with tempfile.TemporaryDirectory() as folder:
            ready = Path(folder) / "ready"
            child = (
                "import signal,time,pathlib,os; "
                "signal.signal(signal.SIGINT, signal.SIG_IGN); "
                "signal.signal(signal.SIGTERM, signal.SIG_IGN); "
                f"pathlib.Path({str(ready)!r}).write_text(str(os.getpgrp())); "
                "time.sleep(30)"
            )
            wrapper = (
                "import importlib.util,sys,os; "
                f"s=importlib.util.spec_from_file_location('runtime',{str(Path(runtime.__file__))!r}); "
                "r=importlib.util.module_from_spec(s); s.loader.exec_module(r); "
                f"sys.exit(r.run_owned([sys.executable,'-c',{child!r}],os.environ.copy()))"
            )
            process = subprocess.Popen([sys.executable, "-c", wrapper])
            try:
                deadline = time.monotonic() + 5
                while not ready.exists() and time.monotonic() < deadline:
                    time.sleep(0.02)
                self.assertTrue(ready.exists())
                process.send_signal(termination)
                self.assertEqual(process.wait(timeout=10), 130)
                self.assertFalse(runtime.group_running(int(ready.read_text())))
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait()

    def test_return_waits_for_owned_descendant_to_exit(self):
        with tempfile.TemporaryDirectory() as folder:
            group_file = Path(folder) / "group"
            ready_file = Path(folder) / "ready"
            descendant = (
                "import signal,time,pathlib; "
                "signal.signal(signal.SIGTERM, lambda *_: None); "
                f"pathlib.Path({str(ready_file)!r}).touch(); "
                "time.sleep(30)"
            )
            parent = (
                "import subprocess,sys,pathlib,os,time\n"
                f"pathlib.Path({str(group_file)!r}).write_text(str(os.getpgrp()))\n"
                f"subprocess.Popen([sys.executable,'-c',{descendant!r}])\n"
                f"while not pathlib.Path({str(ready_file)!r}).exists(): time.sleep(0.01)\n"
            )
            self.assertEqual(runtime.run_owned([sys.executable, "-c", parent], os.environ.copy()), 0)
            self.assertFalse(runtime.group_running(int(group_file.read_text())))


if __name__ == "__main__":
    unittest.main()

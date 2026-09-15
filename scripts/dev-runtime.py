"""Run all native development sessions with one identity and a shared lock."""
import fcntl
import json
import os
from pathlib import Path
import plistlib
import shutil
import signal
import subprocess
import sys
import time
import unicodedata

ROOT = Path(__file__).resolve().parent.parent
IDENTIFIER = "com.memivy.app.dev"
NAME = "Memivy Dev"


def data_root(home, override, qa=False):
    production = (home / "Library/Application Support/com.memivy.app").resolve()
    development = (home / "Library/Application Support" / IDENTIFIER).resolve()
    if qa and not override:
        raise ValueError("QA requires an explicit absolute MEMIVY_DATA_DIR containing synthetic data.")
    selected = Path(override) if override is not None else development
    if not selected.is_absolute() or ".." in selected.parts:
        raise ValueError("MEMIVY_DATA_DIR must be an absolute path without parent traversal.")
    selected = selected.resolve()
    for protected in [production] + ([development] if qa else []):
        # Reserve case and Unicode aliases on macOS, including future directories.
        # Matching all shared components means one path contains the other.
        if all(unicodedata.normalize("NFD", a.casefold()) == unicodedata.normalize("NFD", b.casefold())
               for a, b in zip(selected.parts, protected.parts)):
            raise ValueError("The selected library overlaps a protected application library.")
    return selected


def app_path(home):
    return home / "Applications" / (NAME + ".app")


def running_bundle(bundle):
    commands = subprocess.check_output(["ps", "-axo", "comm="], text=True).splitlines()
    return any(command.strip().startswith(str(bundle) + "/Contents/MacOS/") for command in commands)


def stop_group(group, interrupted=False):
    stages = ([(signal.SIGINT, 2)] if interrupted else [])
    stages += [(signal.SIGTERM, 3), (signal.SIGKILL, 2)]
    for termination, grace in stages:
        if not group_running(group):
            return
        try:
            os.killpg(group, termination)
        except ProcessLookupError:
            return
        deadline = time.monotonic() + grace
        while group_running(group) and time.monotonic() < deadline:
            time.sleep(0.05)
    if group_running(group):
        raise RuntimeError("Owned development processes did not stop; do not start another session.")


def run_owned(args, env):
    child = None
    interrupted = False

    def stop(_signum, _frame):
        nonlocal interrupted
        interrupted = True

    previous = {sig: signal.signal(sig, stop)
                for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP)}
    try:
        if interrupted:
            return 130
        child = subprocess.Popen(args, cwd=ROOT, env=env, start_new_session=True)
        while not interrupted:
            status = child.poll()
            if status is not None:
                return status
            time.sleep(0.05)
        return 130
    finally:
        try:
            if child is not None:
                # Include both the direct child and any Cargo/Vite descendants.
                stop_group(child.pid, interrupted)
                child.wait(timeout=1)
        finally:
            for sig, handler in previous.items():
                signal.signal(sig, handler)


def group_running(group):
    rows = subprocess.check_output(["ps", "-axo", "pgid=,stat="], text=True).splitlines()
    return any(parts[0] == str(group) and not parts[1].startswith("Z")
               for row in rows if len(parts := row.split()) >= 2)


def session(args):
    home = Path.home()
    qa = "--qa" in args
    args = [arg for arg in args if arg != "--qa"]
    if sys.platform != "darwin":
        raise ValueError("Native development requires macOS.")
    selected = data_root(home, os.environ.get("MEMIVY_DATA_DIR"), qa)
    # This lock is shared by every checkout. Never delete the lock inode.
    state = home / "Library/Caches" / IDENTIFIER
    state.mkdir(parents=True, exist_ok=True)
    with (state / "native-session.lock").open("a+") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise ValueError("Memivy Dev is owned by another session. Wait for it to exit, then retry.") from None
        if running_bundle(app_path(home)):
            raise ValueError("Memivy Dev is already running. Quit it normally before starting a session.")
        env = dict(os.environ, MEMIVY_DATA_DIR=str(selected), MEMIVY_DEV_SESSION=str(os.getpid()))
        # Preparation is inside the lock, so concurrent sessions cannot replace sidecars.
        status = run_owned([args[0], str(ROOT / "scripts/prepare-mcp.mjs"), "--debug"], env)
        if status:
            return status
        print(f"{NAME}: {app_path(home)}\nLibrary: {selected}", flush=True)
        # Explicit QA libraries and persistent Dev drafts are never deleted here.
        return run_owned(args, env)


def run(args):
    if not os.environ.get("MEMIVY_DEV_SESSION"):
        raise ValueError("Start native development with npm run dev:app or npm run qa:app.")
    executable = Path(args[0]).resolve(strict=True)
    if executable.name != "memivy":
        raise ValueError("The development runner only accepts the Memivy executable.")
    data_root(Path.home(), os.environ.get("MEMIVY_DATA_DIR"))
    bundle = app_path(Path.home())
    if running_bundle(bundle):
        raise ValueError("The previous Memivy Dev process has not exited; refusing to replace it.")
    contents = bundle / "Contents"
    binaries = contents / "MacOS"
    resources = contents / "Resources"
    for folder in [binaries, resources]:
        folder.mkdir(parents=True, exist_ok=True)
    for name in ["memivy-mcp", "memivy-speech", "memivy-embedding"]:
        shutil.copy2(ROOT / "src-tauri/binaries" / (name + "-aarch64-apple-darwin"), binaries / name)
    shutil.copy2(executable, binaries / NAME)
    shutil.copy2(ROOT / "src-tauri/icons/memivy.icns", resources / "memivy.icns")
    for locale in ["en.lproj", "zh-Hans.lproj"]:
        shutil.copytree(ROOT / "src-tauri/infoplist" / locale, resources / locale, dirs_exist_ok=True)
    with (ROOT / "src-tauri/Info.plist").open("rb") as source:
        info = plistlib.load(source)
    version = json.loads((ROOT / "package.json").read_text())["version"]
    info.update(CFBundleIdentifier=IDENTIFIER, CFBundleName=NAME, CFBundleDisplayName=NAME,
                CFBundleExecutable=NAME, CFBundlePackageType="APPL", CFBundleVersion=version,
                CFBundleShortVersionString=version, CFBundleIconFile="memivy.icns",
                LSMinimumSystemVersion="26.0", NSHighResolutionCapable=True)
    with (contents / "Info.plist").open("wb") as target:
        plistlib.dump(info, target)
    subprocess.run(["codesign", "--force", "--deep", "--sign", "-", "--entitlements",
                    str(ROOT / "src-tauri/Entitlements.plist"), str(bundle)], check=True)
    subprocess.run(["/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister",
                    "-f", str(bundle)], check=True)
    # exec preserves Cargo/Tauri's process ownership and restart behavior.
    os.execv(str(binaries / NAME), [str(binaries / NAME), *args[1:]])


if __name__ == "__main__":
    try:
        if sys.argv[1] == "session":
            sys.exit(session(sys.argv[2:]))
        elif sys.argv[1] == "run":
            run(sys.argv[2:])
        else:
            raise ValueError("Unknown development runtime command.")
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print(f"Memivy Dev: {error}", file=sys.stderr)
        sys.exit(1)

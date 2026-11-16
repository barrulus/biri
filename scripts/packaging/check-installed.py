#!/usr/bin/env python3
"""Exercise the installed compositor's headless startup and IPC, with a deadline."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

identity = json.load(open(sys.argv[1]))
version = subprocess.check_output(['/usr/bin/niri', '--version'], text=True)
assert identity['commit'][:7] in version, 'Installed binary has the wrong source revision'
with tempfile.TemporaryDirectory(prefix='biri-smoke-') as temp:
    root = Path(temp)
    (root / 'config.kdl').write_text('input { keyboard { xkb { layout "us"; }; }; }\n')
    env = {**os.environ, 'XDG_RUNTIME_DIR': temp, 'NIRI_BACKEND': 'headless',
           'LIBGL_ALWAYS_SOFTWARE': '1'}
    with (root / 'compositor.log').open('w+') as log:
        process = subprocess.Popen(['dbus-run-session', '--', '/usr/bin/niri', '-c', str(root / 'config.kdl')],
                                   env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        try:
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                if process.poll() is not None:
                    raise RuntimeError('Installed compositor exited during startup')
                sockets = list(root.glob('niri.*.sock'))
                if sockets:
                    client_env = {**env, 'NIRI_SOCKET': str(sockets[0])}
                    result = subprocess.run(['/usr/bin/niri', 'msg', '--json', 'outputs'], env=client_env,
                                            capture_output=True, text=True, timeout=5)
                    if result.returncode == 0:
                        outputs = json.loads(result.stdout)
                        assert outputs, 'Headless compositor did not create an output'
                        print(result.stdout)
                        quit_result = subprocess.run(
                            ['/usr/bin/niri', 'msg', 'action', 'quit', '--skip-confirmation'],
                            env=client_env, capture_output=True, text=True, timeout=5)
                        # Shutdown can close the IPC connection before its reply is written.
                        # Only tolerate that specific EOF; still require a clean process exit.
                        if quit_result.returncode:
                            assert 'EOF while parsing a value' in quit_result.stderr, quit_result.stderr
                        assert process.wait(timeout=10) == 0, 'Compositor did not exit cleanly'
                        break
                time.sleep(0.2)
            else:
                raise TimeoutError('Installed compositor never answered IPC')
        finally:
            if process.poll() is None:
                import signal
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait()
            log.seek(0)
            print(log.read())

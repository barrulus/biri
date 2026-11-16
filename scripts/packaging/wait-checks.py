#!/usr/bin/env python3
"""Gate publication on the existing CI and Nix workflows for this exact commit."""
import json
import os
import re
import subprocess
import sys
import time

commit = sys.argv[1]
assert re.fullmatch(r'[0-9a-f]{40}', commit)
repository = os.environ['GH_REPO']
assert re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repository)
pending = {'ci.yml', 'nix.yml'}
deadline = time.monotonic() + 7200
while pending and time.monotonic() < deadline:
    for workflow in sorted(pending):
        data = subprocess.check_output(['gh', 'api',
            f'repos/{repository}/actions/workflows/{workflow}/runs?head_sha={commit}&per_page=20'], text=True)
        runs = [r for r in json.loads(data)['workflow_runs']
                if r['head_sha'] == commit and r['head_branch'] == 'main'
                and r['event'] in ('push', 'workflow_dispatch')]
        if not runs:
            print(f'Waiting for {workflow} on {commit}', flush=True)
            continue
        latest = max(runs, key=lambda r: r['id'])
        if latest['status'] == 'completed':
            if latest['conclusion'] != 'success':
                raise SystemExit(f"Refusing publication: {workflow} concluded {latest['conclusion']}: {latest['html_url']}")
            pending.remove(workflow)
            print(f"Passed: {latest['html_url']}", flush=True)
    if pending:
        time.sleep(30)
if pending:
    raise SystemExit(f'Timed out waiting for {sorted(pending)}')

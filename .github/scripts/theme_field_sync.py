#!/usr/bin/env python3
"""Mirror `theme:` labels onto the Biri project board's Theme field.

Idempotent. Needs `gh` authenticated with the `project` scope
(`gh auth refresh -s project`). Adapted from the Fantasy-Map-Generator setup.
"""
import json, subprocess, sys

def gql(query, **variables):
    cmd = ["gh", "api", "graphql", "-f", "query=" + query]
    for k, v in variables.items():
        cmd += ["-f", f"{k}={v}"]
    r = subprocess.run(cmd, capture_output=True, text=True)
    d = json.loads(r.stdout or "{}")
    if r.returncode or "errors" in d:
        raise SystemExit("GQL FAIL: " + (r.stderr or json.dumps(d.get("errors")))[:500])
    return d["data"]

# label slug -> (option name, color)
OPTS = {
 "theme: shaders": ("Shaders", "PURPLE"),
 "theme: overview": ("Overview", "BLUE"),
 "theme: workspaces": ("Workspaces", "GREEN"),
 "theme: floating": ("Floating", "ORANGE"),
 "theme: layout": ("Layout", "GREEN"),
 "theme: window-rules": ("Window rules", "RED"),
 "theme: input-binds": ("Input & binds", "PINK"),
 "theme: outputs": ("Outputs", "BLUE"),
 "theme: ipc": ("IPC", "PURPLE"),
 "theme: capture": ("Capture", "PINK"),
 "theme: upstream-sync": ("Upstream sync", "YELLOW"),
 "theme: project": ("Project", "GRAY"),
}

proj = gql('{user(login:"barrulus"){projectV2(number:5){id fields(first:30){nodes{... on ProjectV2SingleSelectField{id name options{id name}}}}}}}')["user"]["projectV2"]
PID = proj["id"]
existing = {f["name"]: f for f in proj["fields"]["nodes"] if f}

if "Theme" not in existing:
    opts = ",".join('{name:%s,color:%s,description:""}' % (json.dumps(n), c) for n, c in OPTS.values())
    gql('mutation{createProjectV2Field(input:{projectId:"%s",dataType:SINGLE_SELECT,name:"Theme",singleSelectOptions:[%s]}){projectV2Field{... on ProjectV2SingleSelectField{id}}}}' % (PID, opts))
    print("Theme field created")
    proj = gql('{user(login:"barrulus"){projectV2(number:5){id fields(first:30){nodes{... on ProjectV2SingleSelectField{id name options{id name}}}}}}}')["user"]["projectV2"]
    existing = {f["name"]: f for f in proj["fields"]["nodes"] if f}
else:
    print("Theme field already exists")

TF = existing["Theme"]
OID = {o["name"]: o["id"] for o in TF["options"]}

# fetch all items with labels and current Theme value
items, cursor = [], "null"
while True:
    d = gql('{user(login:"barrulus"){projectV2(number:5){items(first:100,after:%s){pageInfo{hasNextPage endCursor}nodes{id fieldValueByName(name:"Theme"){... on ProjectV2ItemFieldSingleSelectValue{name}} content{... on Issue{labels(first:10){nodes{name}}} ... on PullRequest{labels(first:10){nodes{name}}}}}}}}}' % cursor)["user"]["projectV2"]["items"]
    items += d["nodes"]
    if not d["pageInfo"]["hasNextPage"]: break
    cursor = json.dumps(d["pageInfo"]["endCursor"])

muts, skipped = [], 0
for it in items:
    labels = [l["name"] for l in (it.get("content") or {}).get("labels", {}).get("nodes", [])]
    tl = next((l for l in labels if l in OPTS), None)
    if not tl: skipped += 1; continue
    want = OPTS[tl][0]
    cur = (it.get("fieldValueByName") or {}).get("name")
    if cur == want: continue
    muts.append((it["id"], OID[want]))

for s in range(0, len(muts), 20):
    chunk = muts[s:s+20]
    body = " ".join('m%d:updateProjectV2ItemFieldValue(input:{projectId:"%s",itemId:"%s",fieldId:"%s",value:{singleSelectOptionId:"%s"}}){projectV2Item{id}}'
                    % (n, PID, iid, TF["id"], oid) for n, (iid, oid) in enumerate(chunk))
    gql("mutation{" + body + "}")
    print(f"set {min(s+20, len(muts))}/{len(muts)}")
print(f"done: {len(muts)} set, {skipped} items without theme label (left empty)")

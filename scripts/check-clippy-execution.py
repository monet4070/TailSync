#!/usr/bin/env python3
"""Independent scratch negative probe: a cached lint run is not absence of lint."""
import json, subprocess, tempfile
from pathlib import Path
with tempfile.TemporaryDirectory(prefix='tailsync-clippy-probe-') as tmp:
    root=Path(tmp);(root/'src').mkdir();(root/'Cargo.toml').write_text('[package]\nname="tailsync-lint-probe"\nversion="0.0.0"\nedition="2021"\n[workspace]\n')
    (root/'src/lib.rs').write_text('#![deny(clippy::eq_op)]\npub fn probe(x: u32) -> bool { x == x }\n')
    cmd=['cargo','clippy','--offline','--message-format=json','--manifest-path',str(root/'Cargo.toml'),'--target-dir',str(root/'target'),'--','-D','warnings']
    result=subprocess.run(cmd,capture_output=True,text=True)
    messages=[]
    for line in result.stdout.splitlines():
        try: obj=json.loads(line)
        except ValueError: continue
        if obj.get('reason')=='compiler-message' and (obj['message'].get('code') or {}).get('code')=='clippy::eq_op': messages.append(obj['message'])
    # Emit actual JSON diagnostics as well as a small assertion record.
    print(result.stdout,end='');print(result.stderr,end='')
    ok=result.returncode!=0 and bool(messages)
    print(json.dumps(dict(probe='scratch-negative-clippy',lint='clippy::eq_op',scratch_exit=result.returncode,diagnostics=len(messages),status='PASS' if ok else 'FAIL')))
    raise SystemExit(0 if ok else 1)

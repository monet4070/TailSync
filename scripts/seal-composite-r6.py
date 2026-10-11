#!/usr/bin/env python3
"""Seal only complete, unchanged-source R6 local evidence; native remains separate."""
import argparse, hashlib, importlib.util, json, re, subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('evidence',ROOT/'scripts/composite-evidence.py');ev=importlib.util.module_from_spec(spec);spec.loader.exec_module(ev)
p=argparse.ArgumentParser();p.add_argument('run',type=Path);a=p.parse_args();run=a.run.resolve()
if (run/'evidence-index.json').exists(): raise SystemExit('sealed run is immutable; use --verify to inspect it')
data=ev.verify_r6(run, sealed=False)
source=ev.fingerprint()
matrix=json.loads((run/'matrix.json').read_text());checks={c['id']:c for c in data['checks']}
# Update dirty-diff inventory after documentation convergence; tested code hash is unchanged.
(run/'source-fingerprint.json').write_text(json.dumps(source,indent=2)+'\n')
files=subprocess.check_output(['git','ls-files','--cached','--others','--exclude-standard','-z'],cwd=ROOT).decode().split('\0')
docs={n:ev.digest(ROOT/n) for n in sorted(set(files)) if n and n.endswith('.md') and not n.startswith('artifacts/') and (ROOT/n).is_file()}
(run/'document-fingerprint.json').write_text(json.dumps(dict(schema=1,source_sha256=source['source_sha256'],files=docs),indent=2)+'\n')
share=run/'shareable-logs';share.mkdir(exist_ok=True)
for log in (run/'logs').glob('*.log'):
 text=log.read_text(errors='replace').replace(str(ROOT),'<checkout>').replace(str(Path.home()),'<home>')
 text=re.sub(r'/var/folders/[^\s"\']+', '<scratch>', text)
 (share/log.name).write_text(text)
status=dict(schema=1,state='LOCAL_COMPLETE',source_sha256=source['source_sha256'],required_commands=len(matrix['required']),passed_commands=len(checks),unique_compiler_warnings=data['unique_compiler_warnings'],native_windows='UNKNOWN',windows_portable='UNKNOWN',windows_installer='UNKNOWN',full_app='UNKNOWN',target_apps='UNKNOWN',two_devices='UNKNOWN',release='NO-GO',scope='macOS local required tests/builds/package static/private-board gates; installed Windows and R7 acceptance remain UNKNOWN')
(run/'status.json').write_text(json.dumps(status,indent=2)+'\n')
# Seal inventories, logs, results and probe artifacts, excluding package directories
# whose complete file inventories are already in package-manifest.json.
sealed={str(p.relative_to(run)):ev.digest(p) for p in sorted(run.rglob('*')) if p.is_file() and p.name!='evidence-index.json' and not any(x.endswith('.app') for x in p.relative_to(run).parts)}
(run/'evidence-index.json').write_text(json.dumps(dict(schema=1,source_sha256=source['source_sha256'],files=sealed),indent=2)+'\n')
print(json.dumps(status))

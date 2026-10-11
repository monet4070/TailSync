#!/usr/bin/env python3
"""Preserve the first Iroh failure and run the pristine core in this host/network."""
import argparse, datetime, hashlib, json, platform, subprocess
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('run',type=Path);p.add_argument('--baseline',type=Path,default=Path('../repair-snapshots/baseline-f719d58'));a=p.parse_args();baseline=a.baseline.resolve()
assert not subprocess.check_output(['git','diff','HEAD','--','shared/rust-core/src'],cwd=baseline)
source=baseline/'shared/rust-core/src/iroh_transport.rs';log=a.run/'logs/iroh-baseline.log'
if log.exists():raise SystemExit('baseline log already exists; preserve it and use a new run id')
cmd=['node',str(Path('scripts/run-isolated-tests.mjs').resolve()),'--','cargo','test','--locked','--offline','-p','tailsync-core','--lib','iroh_transport::tests','--','--test-threads=1','--nocapture']
with log.open('wb') as output:r=subprocess.run(cmd,cwd=baseline,stdout=output,stderr=subprocess.STDOUT)
record=dict(schema=1,utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),host=platform.platform(),baseline_head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=baseline,text=True).strip(),baseline_iroh_source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),command=cmd,exit_code=r.returncode,log_sha256=hashlib.sha256(log.read_bytes()).hexdigest(),note='Immediate same-host/network baseline; original addresses and original test deadlines retained. This comparison does not fix a product budget or waive a failure.')
(a.run/'iroh-baseline-comparison.json').write_text(json.dumps(record,indent=2)+'\n');print(json.dumps(record))
raise SystemExit(0 if b'test result:' in log.read_bytes() else 1)

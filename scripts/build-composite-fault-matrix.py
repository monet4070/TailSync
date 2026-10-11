#!/usr/bin/env python3
import argparse, json, re
from pathlib import Path
import importlib.util
ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('composite_evidence', ROOT/'scripts/composite-evidence.py')
ev=importlib.util.module_from_spec(spec);spec.loader.exec_module(ev)
p=argparse.ArgumentParser();p.add_argument('run',type=Path);a=p.parse_args();cases=[]
for log in sorted((a.run/'logs').glob('receiver-faults.*.log')):
 for raw in re.findall(r'R6_FAULT (\{[^\n]+\})',log.read_text()):
  item=json.loads(raw);item.update(evidence=str(log.relative_to(a.run)),log_sha256=ev.digest(log),kind='production-receiver-with-counting-adapter');cases.append(item)
for log in [a.run/'regressions/logs/RR-NATIVE.log', a.run/'package-probes/native-faults.json']:
 if not log.exists(): continue
 data=json.loads(log.read_text())
 for key in ['RR-03-contract','RR-04-contract','R6-before-spawn','R6-output-exceeded']:
  entry=data[key];item=entry['probe'];item.update(fault_point=entry['seam'],terminal=item['outcome'],status='PASS' if entry['satisfied'] else 'FAIL',kind='private-board-production-supervisor',evidence=str(log.relative_to(a.run)),log_sha256=ev.digest(log));cases.append(item)
 for key in ['RR-07']:
  entry=data[key];cases.append(dict(fault_point=entry['seam'],terminal='FileSelection',native_attempts=0,history_count=None,child_reaped=None,budget_released=None,scope='native capture shape; no receiver publication or child supervision',status='PASS' if entry['classified_as_file_selection'] else 'FAIL',kind='private-board-shape',evidence=str(log.relative_to(a.run)),log_sha256=ev.digest(log)))
for item in cases:
 if 'status' not in item:item['status']='PASS' if item['native_attempts']<=1 and item['budget_released'] else 'FAIL'
 if item['kind']=='private-board-production-supervisor':
  spawned=item.get('native_attempts',0) > 0
  if not item.get('budget_released') or (spawned and not item.get('child_reaped')) or item.get('native_attempts') not in (0,1):item['status']='FAIL'
source=json.loads((a.run/'source-fingerprint.json').read_text())['source_sha256']
out=dict(schema=1,source_sha256=source,cases=cases,note='Null means outside that probe seam, not PASS. Receiver rows measure history and attempt counters; private-board rows measure actual child reap and supervisor memory release. Synthetic fixtures contain no user clipboard bodies.')
(a.run/'fault-matrix.json').write_text(json.dumps(out,indent=2)+'\n');print(f'{len(cases)} fault/shape observations')
raise SystemExit(len(cases)<10 or any(i['status']!='PASS' for i in cases))

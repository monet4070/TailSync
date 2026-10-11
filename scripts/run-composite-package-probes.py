#!/usr/bin/env python3
"""Private-board probes against actual new debug/packaged helper bytes."""
import argparse, hashlib, json, subprocess, uuid
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('run',type=Path);a=p.parse_args();run=a.run.resolve()
manifest=json.loads((run/'package-manifest.json').read_text());out=run/'package-probes';out.mkdir(exist_ok=False);records=[]
build=['cargo','build','--locked','--offline','-q','-p','tailsync-core','--example','rtfd_capture_probe'];subprocess.run(build,check=True)
for package in manifest['packages']:
 if package['status']=='UNKNOWN':continue
 assert package['status']=='PASS'
 helper=Path(package['path'])/'clipboard-helper' if package['kind']=='debug' else Path(package['path'])/'Contents/MacOS/clipboard-helper'
 board='tailsync-probe.r6-'+str(uuid.uuid4());snapshot=out/(package['kind']+'-snapshot.bin')
 # Probe source is a synthetic RTFD text + embedded image fixture; never general.
 with snapshot.open('wb') as target:
  generated=subprocess.run([str(helper),'--pasteboard',board,'--probe-rtfd'],stdout=target,stderr=subprocess.PIPE)
 assert generated.returncode==0
 result=subprocess.run(['target/debug/examples/rtfd_capture_probe',str(snapshot),str(helper),board],capture_output=True,text=True)
 log=out/(package['kind']+'.log');log.write_text(result.stdout+result.stderr)
 assert result.returncode==0 and 'Published receipt, stable readback and matching SelfWrite' in result.stdout
 records.append(dict(kind=package['kind'],helper_sha256=hashlib.sha256(helper.read_bytes()).hexdigest(),snapshot_sha256=hashlib.sha256(snapshot.read_bytes()).hexdigest(),log_sha256=hashlib.sha256(log.read_bytes()).hexdigest(),terminal='Published',original_rtfd_preserved=True,portable_images=1,status='PASS'))
helper=run/'TailSync-enabled-native.app/Contents/MacOS/clipboard-helper';fault_out=out/'fault-helpers'
cmd=['python3','tools/repair-probes/native_fault_probe.py','--json','--keep','--out-dir',str(fault_out),'--product-helper',str(helper)]
result=subprocess.run(cmd,capture_output=True,text=True,timeout=180);log=out/'native-faults.json';log.write_text(result.stdout)
(run/'logs/package-fault-stderr.log').write_text(result.stderr);assert result.returncode==0
faults=json.loads(result.stdout)
assert all(faults[k]['satisfied'] for k in ['RR-03-contract','RR-04-contract','R6-before-spawn','R6-output-exceeded']) and faults['RR-07']['classified_as_file_selection']
records.append(dict(kind='packaged-helper-private-board-faults',command=cmd,helper_sha256=hashlib.sha256(helper.read_bytes()).hexdigest(),log_sha256=hashlib.sha256(log.read_bytes()).hexdigest(),fault_helpers={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in fault_out.iterdir() if p.is_file()},status='PASS'))
(run/'package-probes.json').write_text(json.dumps(dict(schema=1,source_sha256=manifest['source_sha256'],checks=records,status='PASS'),indent=2)+'\n')
print(json.dumps(dict(probe='packaged-helpers',checks=len(records),status='PASS',source_sha256=manifest['source_sha256'])))

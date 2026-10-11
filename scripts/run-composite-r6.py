#!/usr/bin/env python3
"""Reproducible local R6 matrix. Every attempt has its own log and metadata."""
import argparse, datetime, json, os, platform, subprocess, time
from pathlib import Path
import importlib.util
ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('composite_evidence', ROOT/'scripts/composite-evidence.py')
ev=importlib.util.module_from_spec(spec);spec.loader.exec_module(ev)
p=argparse.ArgumentParser();p.add_argument('run_id');p.add_argument('--only',nargs='*');p.add_argument('--list',action='store_true');p.add_argument('--list-json',action='store_true');a=p.parse_args()
if not a.run_id or Path(a.run_id).name != a.run_id or a.run_id in ('.', '..'): raise SystemExit('run_id must be one directory name')
run=ROOT/'artifacts/composite-repair'/a.run_id
iso=['node','scripts/run-isolated-tests.mjs','--']; cargo=['cargo','test','--locked','--offline','--message-format=json']
checks=[]
def add(id,cmd,min_tests=0,exact_tests=None,environment=None): checks.append(dict(id=id,command=cmd,min_tests=min_tests,exact_tests=exact_tests,environment=environment or {}))
def core(id,features=None,extra=None,environment=None,exact=None):
 cmd=cargo+['-p','tailsync-core']
 if features: cmd+=['--features',features]
 cmd+=extra or ['--lib'];cmd+=['--','--test-threads=1','--nocapture'];add(id,iso+cmd,1,exact,environment)
core('core-default');core('core-enabled','protocol-composite-clipboard protocol-file-sliding-window protocol-image-compressed-chunks');core('core-repair','repair-regressions test-support protocol-composite-clipboard')
core('runtime-disabled','protocol-composite-clipboard',['--lib','runtime_disabled'],{'TAILSYNC_DISABLE_COMPOSITE_CLIPBOARD':'1'},3)
core('iroh-real-network','protocol-composite-clipboard',['--lib','iroh_transport::tests::real_network_'],exact=2)
add('protocol-runtime',iso+cargo+['-p','tailsync-protocol','-p','tailsync-runtime','--features','tailsync-core/protocol-composite-clipboard','--','--test-threads=1'],1)
add('runtime-repair',iso+cargo+['-p','tailsync-runtime','--features','repair-regressions test-support','--test','repair_regressions','--','--test-threads=1'],1)
core('receiver-faults','repair-regressions test-support protocol-composite-clipboard',['--test','composite_repair_followup'])
for plat in ['macos','windows']:
 for state in ['default','enabled']:
  cmd=cargo+['--manifest-path',f'{plat}/src-tauri/Cargo.toml','--lib']
  if state=='enabled':cmd+=['--features','protocol-composite-clipboard']
  add(f'{plat}-{state}',iso+cmd+['--','--test-threads=1'],1)
add('swift',iso+['swift','test','--package-path','macos/swift-ui'],1)
add('windows-ui',iso+['npm','--prefix','windows','test'],1)
add('windows-build',['npm','--prefix','windows','run','build']);add('windows-lint',['npm','--prefix','windows','run','lint'])
for state in ['default','enabled']:
 args=['--features','tailsync-core/protocol-composite-clipboard'] if state=='enabled' else []
 add(f'workspace-check-{state}',['cargo','check','--locked','--offline','--workspace','--all-targets','--message-format=json']+args)
 add(f'workspace-clippy-{state}',['cargo','clippy','--locked','--offline','--workspace','--all-targets','--message-format=json']+args+['--','-D','warnings'])
for state in ['default','enabled']:
 args=['--features','protocol-composite-clipboard'] if state=='enabled' else []
 add(f'macos-clippy-{state}',['cargo','clippy','--locked','--offline','--manifest-path','macos/src-tauri/Cargo.toml','--all-targets','--message-format=json']+args+['--','-D','warnings'])
add('repair-clippy',['cargo','clippy','--locked','--offline','-p','tailsync-core','-p','tailsync-runtime','--features','repair-regressions test-support tailsync-core/protocol-composite-clipboard','--all-targets','--message-format=json','--','-D','warnings'])
add('contracts',['node','scripts/check-local-contracts.mjs']);add('cross-platform',['node','windows/scripts/check_cross_platform_sync.mjs','--win-root','windows','--mac-root','macos','--core-root','shared/rust-core'])
add('canonical-includes',['node','scripts/check-canonical-includes.mjs']);add('canonical-negative',['node','--test','scripts/check-canonical-includes.test.mjs'],23,23)
add('evidence-parser',['python3','-m','unittest','discover','-s','scripts/tests','-p','test_composite_evidence.py'],12,12)
add('r7-harness',['python3','-m','unittest','discover','-s','scripts/tests','-p','test_composite_r7.py'],1)
add('evidence-validity',['python3','-m','unittest','discover','-s','scripts/tests','-p','test_composite_r6.py'],1)
add('lint-negative',['python3','scripts/check-clippy-execution.py'])
add('build-args',['bash','scripts/check-build-mac-feature-args.sh'])
add('regressions',['bash','scripts/run-repair-regressions.sh',a.run_id+'/regressions'])
add('rollback-proof',iso+['python3','scripts/run-composite-rollback-proof.py',str(run)])
add('windows-helper-host-check',['cargo','check','--locked','--offline','-p','tailsync-clipboard-helper','--target','x86_64-pc-windows-msvc','--features','portable-check','--message-format=json'])
add('fault-matrix',['python3','scripts/build-composite-fault-matrix.py',str(run)])
for state in ['default','enabled']:
 for arch in ['native','universal-apple-darwin']:
  name=f'TailSync-{state}-{arch}.app'
  add(f'macos-build-{state}-{arch}',['/bin/bash','macos/build-mac.sh'],environment={
      'TAILSYNC_APP_BUNDLE':str(run/name), 'TAILSYNC_COMPOSITE_CLIPBOARD':'1' if state=='enabled' else '0',
      'TAILSYNC_MACOS_TARGET':arch})
add('macos-debug-build',['/bin/bash','scripts/build-composite-debug.sh'])
manifest=['python3','scripts/build-composite-package-manifest.py',str(run),'--debug',str(ROOT/'macos/src-tauri/target-macos/debug/tailsync')]
for state in ['default','enabled']:
 for arch in ['native','universal-apple-darwin']:
  manifest+=['--app',f'{state}-{arch}={run}/TailSync-{state}-{arch}.app']
add('package-manifest',manifest)
add('package-probes',iso+['python3','scripts/run-composite-package-probes.py',str(run)])
add('fault-matrix-final',['python3','scripts/build-composite-fault-matrix.py',str(run)])
if a.list_json:
 print(json.dumps(checks));raise SystemExit
if a.list:
 print('\n'.join(c['id'] for c in checks));raise SystemExit
run.mkdir(parents=True,exist_ok=True);(run/'logs').mkdir(exist_ok=True);(run/'commands').mkdir(exist_ok=True)
if (run/'evidence-index.json').exists(): raise SystemExit('sealed run is immutable; use a new run id')
source=ev.fingerprint();source_path=run/'source-fingerprint.json'
if source_path.exists() and json.loads(source_path.read_text())['source_sha256']!=source['source_sha256']:
 raise SystemExit('source changed: create a new run id; do not mix evidence fingerprints')
source_path.write_text(json.dumps(source,indent=2)+'\n')
(run/'environment.json').write_text(json.dumps(dict(platform=platform.platform(),machine=platform.machine(),python=platform.python_version(),isolation='commands use run-isolated-tests; no claim of a user-state backup'),indent=2)+'\n')
selected=[c for c in checks if a.only is None or c['id'] in a.only]
if not selected:raise SystemExit('no selected commands')
(run/'matrix.json').write_text(json.dumps(dict(required=[c['id'] for c in checks],selected=[c['id'] for c in selected],commands=checks),indent=2)+'\n')
for check in selected:
 id=check['id'];attempt=1
 while (run/'commands'/f'{id}.{attempt}.json').exists():attempt+=1
 log=run/'logs'/f'{id}.{attempt}.log'; started=time.time();print(f'RUN {id} attempt {attempt}',flush=True)
 env=os.environ.copy();env.update(check['environment']);env['CARGO_TERM_COLOR']='never';env['NO_COLOR']='1'
 with log.open('wb') as output:
  result=subprocess.run(check['command'],cwd=ROOT,env=env,stdout=output,stderr=subprocess.STDOUT)
 record=dict(check,attempt=attempt,exit_code=result.returncode,started_utc=datetime.datetime.fromtimestamp(started,datetime.timezone.utc).isoformat(),elapsed_s=round(time.time()-started,3),log=str(log.relative_to(run)),source_sha256=source['source_sha256'],host=platform.system())
 (run/'commands'/f'{id}.{attempt}.json').write_text(json.dumps(record,indent=2)+'\n')
 data=ev.generate(run);current=next(c for c in data['checks'] if c['id']==id and c['attempt']==attempt)
 print(f"{current['status']} {id}: exit={result.returncode}, tests={current['executed_tests']}, warnings={current['unique_compiler_warnings']}",flush=True)
 if current['status']=='FAIL' and 'iroh_transport::tests::' in log.read_text(errors='replace') and 'FAILED' in log.read_text(errors='replace'):
  subprocess.run(['python3','scripts/compare-iroh-baseline.py',str(run)],cwd=ROOT,check=True)
 if ev.fingerprint()['source_sha256']!=source['source_sha256']:raise SystemExit('source changed during verification; evidence preserved, cannot seal')
# All attempted failures remain in results even when a subsequent retry passes.
data=ev.generate(run)
raise SystemExit(bool(data['unexpected']))

#!/usr/bin/env python3
"""Inspect freshly built packages; unavailable platforms stay UNKNOWN."""
import argparse, hashlib, json, plistlib, re, shutil, subprocess
from pathlib import Path
import importlib.util
ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('composite_evidence', ROOT/'scripts/composite-evidence.py')
ev=importlib.util.module_from_spec(spec);spec.loader.exec_module(ev)
def snapshot_debug(run, daemon):
 """Copy fresh build outputs before inspecting; never seal a mutable cache path."""
 run=run.resolve()
 if (run/'evidence-index.json').exists(): raise ValueError('sealed run is immutable')
 snapshot=run/'debug-package';snapshot.mkdir(exist_ok=False)
 for name,source in [('tailsync',daemon),('clipboard-helper',daemon.parent/'clipboard-helper')]:
  shutil.copy2(source,snapshot/name)
 return snapshot
def command(args):
 r=subprocess.run(args,capture_output=True,text=True);return dict(command=args,exit_code=r.returncode,stdout=r.stdout,stderr=r.stderr)
def inspect(kind,path,source):
 item=dict(kind=kind,path=str(path.resolve()),source_sha256=source,files=[])
 if kind=='debug':binaries=[path/'tailsync',path/'clipboard-helper'];helper=binaries[-1]
 else:
  binaries=[path/'Contents/MacOS'/n for n in ['TailSync','tailsyncd','clipboard-helper']];helper=binaries[-1]
  item['bundle_minimum_os']=plistlib.loads((path/'Contents/Info.plist').read_bytes())['LSMinimumSystemVersion']
  item['signature']=command(['codesign','--verify','--deep','--strict',str(path)])
  item['deployment']=command(['python3','macos/scripts/check_macos_deployment.py',str(path)])
 for binary in binaries:
  arches=command(['lipo','-archs',str(binary)]); loads=command(['otool','-l',str(binary)])
  item['files'].append(dict(path=str(binary.relative_to(path)),sha256=ev.digest(binary),size=binary.stat().st_size,architectures=arches['stdout'].split(),minimum_os=re.findall(r'\bminos (\S+)',loads['stdout']),signature=command(['codesign','--verify','--strict',str(binary)]) if kind!='debug' else None))
 item['all_files_sha256']={str(p.relative_to(path)):ev.digest(p) for p in sorted(path.rglob('*')) if p.is_file()}
 item['package_sha256']=hashlib.sha256(''.join(f'{p}\0{h}\n' for p,h in item['all_files_sha256'].items()).encode()).hexdigest()
 item['helper_self_check']=command([str(helper),'--self-check'])
 daemon=binaries[0] if kind=='debug' else binaries[1]
 item['resolver']=command([str(daemon),'--tailsync-clipboard-helper-check'])
 try:resolved=json.loads(item['resolver']['stdout'])
 except ValueError:resolved={}
 item['expected_architectures']=['arm64','x86_64'] if 'universal' in kind else ['arm64']
 item['architectures_match']=all(set(f['architectures'])==set(item['expected_architectures']) for f in item['files'])
 item['feature_matches']=resolved.get('composite_compiled') == (kind=='debug' or kind.startswith('enabled'))
 item['resolver_matches_package']=resolved.get('helper_path')==str(helper.resolve())
 item['helper_protocol']=resolved.get('expected_protocol')
 item['status']='PASS' if item['resolver']['exit_code']==0 and item['helper_self_check']['exit_code']==0 and item['resolver_matches_package'] and item['helper_protocol']==4 and resolved.get('healthy') is True and 'protocol=4' in item['helper_self_check']['stdout'] and item['architectures_match'] and item['feature_matches'] and all(f['minimum_os'] and all(tuple(map(int,v.split('.'))) <= (13,0,0) for v in f['minimum_os']) for f in item['files']) and (kind=='debug' or item['signature']['exit_code']==0 and item['deployment']['exit_code']==0) else 'FAIL'
 return item

def main():
 p=argparse.ArgumentParser();p.add_argument('run',type=Path);p.add_argument('--app',action='append',default=[]);p.add_argument('--debug',type=Path);a=p.parse_args()
 if (a.run/'evidence-index.json').exists(): raise SystemExit('sealed run is immutable')
 source=json.loads((a.run/'source-fingerprint.json').read_text())['source_sha256'];entries=[]
 for app in a.app:
  kind,value=app.split('=',1);path=ev.contained(a.run,value)
  entries.append(inspect(kind,path,source))
 if a.debug:entries.append(inspect('debug',snapshot_debug(a.run,a.debug),source))
 for kind in ['windows-portable','windows-installer']:
  entries.append(dict(kind=kind,status='UNKNOWN',path=None,source_sha256=source,architectures=None,minimum_os=None,helper_protocol=None,signature=None,sha256=None,reason='macOS host has no Windows linker/installer/native execution environment; host MSVC cargo check is recorded separately, never substituted'))
 data=dict(schema=1,source_sha256=source,packages=entries,release='NO-GO')
 (a.run/'package-manifest.json').write_text(json.dumps(data,indent=2)+'\n');print(json.dumps({e['kind']:e['status'] for e in entries}))
 return any(e['status']=='FAIL' for e in entries)

if __name__=='__main__':
 raise SystemExit(main())

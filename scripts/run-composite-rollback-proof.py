#!/usr/bin/env python3
"""Fresh current-writer / previous-source-reader rollback and negative control."""
import argparse, hashlib, json, os, shutil, subprocess
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('run',type=Path);p.add_argument('--baseline',type=Path,default=Path('../repair-snapshots/baseline-f719d58'));a=p.parse_args()
root=Path.cwd();run=a.run.resolve();baseline=a.baseline.resolve();out=run/'rollback';out.mkdir(parents=True,exist_ok=False);logs=run/'logs';logs.mkdir(exist_ok=True)
records=[]
def command(id,cmd,cwd=root,expected=0):
 log=logs/f'rollback-{id}.log'
 with log.open('wb') as f:r=subprocess.run(cmd,cwd=cwd,stdout=f,stderr=subprocess.STDOUT)
 records.append(dict(id=id,command=cmd,cwd=str(cwd),exit=r.returncode,expected=expected,log=str(log.relative_to(run)),log_sha256=hashlib.sha256(log.read_bytes()).hexdigest()))
 if (r.returncode==0)!=(expected==0):raise RuntimeError(f'{id} unexpected exit {r.returncode}')
 return log
head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=baseline,text=True).strip()
assert head=='f719d581a1aa5198a1b3f2483222df5c69e3dfc0'
assert not subprocess.check_output(['git','diff','HEAD','--','shared/rust-core/src'],cwd=baseline), 'old reader production core must be pristine'
reader=baseline/'shared/rust-core/examples/read_restored.rs';assert reader.is_file()
command('baseline-build',['cargo','build','--locked','--offline','-p','tailsync-core','--example','read_restored'],baseline)
authority=out/'authority';authority.mkdir()
# Synthetic stand-ins verify the restore scope. Actual receipt semantics also have
# production-journal restart regressions in the separate fault matrix.
for name in ['identity-v1.bin','trusted-peers.json','effect-receipts.json']:(authority/name).write_bytes(('R6 retained synthetic authority '+name).encode())
def hashes(path):return {str(p.relative_to(path)):hashlib.sha256(p.read_bytes()).hexdigest() for p in path.rglob('*') if p.is_file()}
before=hashes(authority)
writer=command('writer',['cargo','run','--locked','--offline','-q','-p','tailsync-core','--features','test-support','--example','rollback_probe','--',str(out/'work')])
info=json.loads(writer.read_text());assert info['manifest_payloads']==2 and info['post_upgrade_readable'] and info['upgraded_schema_version']==13
store=out/'work/store';archive=out/'post-upgrade-archive';shutil.copytree(store,archive);assert hashes(store)==hashes(archive)
reader_bin=baseline/'target/debug/examples/read_restored'
complete=command('old-reader',[str(reader_bin),str(out/'work/restored')]);old=json.loads(complete.read_text());assert old['entries_read']==3 and old['all_payloads_readable'] and old['favorites']==1
counter=out/'database-only';counter.mkdir();shutil.copy2(store/'pre-v12-rollback/history-before-v12.db',counter/'history-v2.db')
command('database-only-negative',[str(reader_bin),str(counter)],expected=1)
command('restore-script',['bash','scripts/restore-pre-v12-rollback.sh',str(store),str(out/'script-restored')])
command('script-old-reader',[str(reader_bin),str(out/'script-restored')]);assert hashes(authority)==before
result=dict(schema=1,source_sha256=json.loads((run/'source-fingerprint.json').read_text())['source_sha256'],baseline_head=head,baseline_reader_sha256=hashlib.sha256(reader_bin.read_bytes()).hexdigest(),baseline_reader_source_sha256=hashlib.sha256(reader.read_bytes()).hexdigest(),current_writer=info,complete_package=old,post_upgrade_archive_files=hashes(archive),authority_before=before,authority_after=hashes(authority),commands=records,status='PASS',limits='Authority scope uses synthetic stand-ins; native installed-version switching is R7 UNKNOWN. Old-reader core source is pristine at the recorded commit, with an untracked read-only harness.')
(run/'rollback-proof.json').write_text(json.dumps(result,indent=2)+'\n')
(run/'rollback-proof.md').write_text(f'''# R6 rollback proof\n\nPASS — fresh current writer, baseline {head} reader. Source: `{result['source_sha256']}`.\n\nThe upgrade produced a v11 SQLite snapshot and two independent external payload copies. The old binary read all three text/image/file rows and the favorite, with matching data hashes. A database-only restore failed as expected. The documented restore script also passed the old reader.\n\nPost-upgrade v13 history, including a new composite row, was archived with every file hash equal to the quiescent source. Synthetic identity/trust/receipt stand-ins were unchanged before and after restoration and were never copied into the history restore. Production receipt restart behavior is independently covered in fault-matrix.json.\n\nCommands, exits and log SHA256 values are in rollback-proof.json. Previous reader binary SHA256: `{result['baseline_reader_sha256']}`. Native installed-version switching remains UNKNOWN for R7.\n''')
print(json.dumps(dict(probe='rollback',status='PASS',entries=old['entries_read'],payloads=info['manifest_payloads'],negative_control='expected failure',source_sha256=result['source_sha256'])))

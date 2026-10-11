#!/usr/bin/env python3
"""Isolated v12 production writer/reader against the v13 upgrade and rollback.

The old source is required explicitly and is never edited. Diagnostic manifests
and binaries live outside the checkout; archived manifests have a .txt suffix.
"""
import argparse, hashlib, json, os, shutil, subprocess, tempfile
from pathlib import Path

OLD = r'''
use tailsync_core::{db::HistoryDB,composite::{Completeness,Mime,PortableBundle,Representation},protocol::native_clipboard::{NativeSnapshot,NativeFormat,FormatState,SnapshotState}};
fn main()->Result<(),Box<dyn std::error::Error>> {
 let args:Vec<_>=std::env::args().collect();let root=std::path::PathBuf::from(&args[2]);
 std::env::set_var("TAILSYNC_STORAGE_DIR",&root);std::env::set_var("TAILSYNC_DATA_DIR",root.join("probe-authority"));
 let mut db=if args[1]=="create" {HistoryDB::open_isolated_for_test(&root)?} else {HistoryDB::new()?};
 if args[1]=="create" {
  db.add_text("v12 text fixture","self")?;db.add_file("v12.bin",b"v12 file bytes","self")?;
  let bundle=PortableBundle {representations:vec![Representation {mime:Mime::Text,bytes:vec![b't';128*1024]}],resources:vec![],bindings:vec![],completeness:Completeness::Complete};
  let native=NativeSnapshot {before:0,after:0,items:1,state:SnapshotState::Stable,formats:vec![NativeFormat {item:0,name:"com.apple.flat-rtfd".into(),state:FormatState::Captured,bytes:vec![b'r';128*1024]}]};
  let id=db.add_composite(&bundle,Some(&native),"self")?;db.set_favorite(id,true)?;
 }
 let rows=db.get_all(None,None,50,0)?;let mut hashes=vec![];
 for row in rows {let bytes=db.get_data(row.id)?;let native=if row.entry_type=="composite" {db.get_composite_native(row.id)?.unwrap().encode()?} else {vec![]};hashes.push(serde_json::json!({"id":row.id,"type":row.entry_type,"favorite":row.pinned,"data_hash":row.data_hash,"data_sha256":blake3::hash(&bytes).to_hex().to_string(),"native_digest":blake3::hash(&native).to_hex().to_string()}));}
 println!("{}",serde_json::to_string(&hashes)?);Ok(())
}
'''
NEW = r'''
use tailsync_core::{db::{HistoryDB,MigrationPolicy},composite::{Completeness,Mime,PortableBundle,Representation}};
fn main()->Result<(),Box<dyn std::error::Error>> {
 let args:Vec<_>=std::env::args().collect();let root=std::path::PathBuf::from(&args[1]);let restore=std::path::PathBuf::from(&args[2]);
 std::env::set_var("TAILSYNC_STORAGE_DIR",&root);std::env::set_var("TAILSYNC_DATA_DIR",root.join("probe-authority"));
 let mut db=HistoryDB::open_with_policy_for_probe(&root,MigrationPolicy::CompositeAllowed)?;
 assert_eq!(db.schema_version_for_probe()?,13);
 let before=db.get_all(None,None,50,0)?;assert_eq!(before.len(),3);
 for row in &before {assert!(db.composite_identity(row.id).is_err() || db.composite_identity(row.id)?.is_none());db.get_data(row.id)?;}
 let b=PortableBundle {representations:vec![Representation {mime:Mime::Text,bytes:b"post-v13 preserved".to_vec()}],resources:vec![],bindings:vec![],completeness:Completeness::Complete};
 let id=db.add_composite(&b,None,"self")?;assert!(db.composite_identity(id)?.is_some());drop(db);
 let manifest=tailsync_core::db::restore_v13_rollback_package(&root,&restore)?;
 assert_eq!(manifest.source_schema_version,12);assert_eq!(manifest.manifest_version,2);assert!(!manifest.payloads.is_empty());
 let old_view=HistoryDB::open_with_policy_for_probe(&restore,MigrationPolicy::LegacyOnly)?;
 assert_eq!(old_view.schema_version_for_probe()?,12);assert_eq!(old_view.get_all(None,None,50,0)?.len(),3);drop(old_view);
 let current=HistoryDB::open_with_policy_for_probe(&root,MigrationPolicy::LegacyOnly)?;assert_eq!(current.get_composite(id)?,b);
 println!("{}",serde_json::json!({"schema":13,"snapshot_schema":12,"manifest_version":2,"payloads":manifest.payloads.len(),"post_upgrade_row_preserved":true}));Ok(())
}
'''

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('run',type=Path);parser.add_argument('--old-source',required=True,type=Path)
    args=parser.parse_args();root=Path(__file__).resolve().parents[1];run=args.run.resolve();run.mkdir(parents=True,exist_ok=True)
    old=args.old_source.resolve();old_core=old/'shared/rust-core'
    if 'const SCHEMA_VERSION: i64 = 12;' not in (old_core/'src/db.rs').read_text(): raise RuntimeError('old source must actually be schema v12')
    def source_files(source_root):
        return {str(p.relative_to(source_root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted((source_root/'shared').rglob('*')) if p.is_file() and p.suffix in ['.rs','.toml']}
    current_source_hashes=source_files(root)
    records=[]
    with tempfile.TemporaryDirectory(prefix='tailsync-v13-proof-') as scratch:
        scratch=Path(scratch)
        def harness(name,source,core):
            path=scratch/name;(path/'src').mkdir(parents=True)
            manifest=f'''[package]\nname = "tailsync-v13-{name}"\nversion = "0.0.0"\nedition = "2021"\npublish = false\n[workspace]\n[dependencies]\ntailsync-core = {{ path = {json.dumps(str(core))}, features = ["test-support", "protocol-composite-clipboard"] }}\nserde_json = "1"\nblake3 = "1"\n'''
            (path/'Cargo.toml').write_text(manifest);(path/'src/main.rs').write_text(source)
            (run/f'{name}-Cargo.toml.txt').write_text(manifest);(run/f'{name}-main.rs').write_text(source)
            return path
        reader=harness('v12',OLD,old_core);writer=harness('v13',NEW,root/'shared/rust-core')
        def command(name,cmd,expected=0):
            log=run/f'{name}.log'
            with log.open('wb') as output: result=subprocess.run(cmd,cwd=root,stdout=output,stderr=subprocess.STDOUT)
            records.append(dict(name=name,command=cmd,exit=result.returncode,expected=expected,log=log.name,sha256=hashlib.sha256(log.read_bytes()).hexdigest()))
            if (result.returncode==0)!=(expected==0): raise RuntimeError(f'{name} unexpected exit {result.returncode}: {log}')
            return log.read_text().strip()
        for name,path in [('old-build',reader),('new-build',writer)]:
            command(name,['cargo','build','--offline','--manifest-path',str(path/'Cargo.toml')])
            (run/f'{name}-Cargo.lock.txt').write_bytes((path/'Cargo.lock').read_bytes())
        old_bin=reader/'target/debug/tailsync-v13-v12';new_bin=writer/'target/debug/tailsync-v13-v13'
        store=run/'v12-store';restore=run/'restored-v12'
        before=json.loads(command('old-create',[str(old_bin),'create',str(store)]))
        upgrade=json.loads(command('upgrade-and-restore',[str(new_bin),str(store),str(restore)]))
        after=json.loads(command('old-read',[str(old_bin),'read',str(restore)]));assert before==after
        command('old-rejects-v13',[str(old_bin),'read',str(store)],expected=1)
        negative=run/'database-only';negative.mkdir();shutil.copy2(store/'pre-v13-rollback/history-before-v13.db',negative/'history-v2.db')
        command('database-only-negative',[str(old_bin),'read',str(negative)],expected=1)
        # Preserve all post-upgrade history while proving the rollback in a new root.
        archive=run/'post-v13-archive';shutil.copytree(store,archive)
        def files(path): return {str(p.relative_to(path)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(path.rglob('*')) if p.is_file()}
        assert files(store)==files(archive)
        source_hashes={str(p.relative_to(old)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted((old/'shared').rglob('*')) if p.is_file() and p.suffix in ['.rs','.toml']}
        assert current_source_hashes==source_files(root), 'current sources changed while the proof ran'
        result=dict(status='PASS',current_source_hashes=current_source_hashes,old_source=str(old),old_source_hashes=source_hashes,old_binary_sha256=hashlib.sha256(old_bin.read_bytes()).hexdigest(),before=before,after=after,upgrade=upgrade,post_upgrade_archive_files=files(archive),commands=records,limits='Synthetic production DB/reader proof; no installed App, clipboard, power-cut or actual disk-exhaustion acceptance.')
        (run/'proof.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps({k:result[k] for k in ['status','upgrade','limits']}))

if __name__=='__main__': main()

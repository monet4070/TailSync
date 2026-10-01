"""Reproduce in an isolated copy. Pass copy root and evidence directory explicitly."""
from pathlib import Path
import subprocess, os, hashlib, json, sys
root, evidence = map(lambda p: Path(p).resolve(), sys.argv[1:3])
assert root != Path.cwd().resolve() and (root / 'Cargo.toml').exists() and not (root / '.git').exists(), 'Use an isolated copy without Git metadata'
env = os.environ.copy()
env.update(TAILSYNC_DATA_DIR=str(root.parent/'data'), TAILSYNC_V1_DATA_DIR=str(root.parent/'v1'))
results = []
def rollback(s):
 a=s.index('fn remove_unreferenced_persisted_files('); b=s.index('\npub struct HistoryDB',a)
 return s[:a]+'''fn remove_unreferenced_persisted_files(db: &HistoryDB, persisted: &[(Vec<u8>, PathBuf)]) {
    for (reference, path) in persisted {
        let live: i64 = db.conn.query_row("SELECT COUNT(*) FROM history WHERE data=?1", params![reference], |r| r.get(0)).unwrap();
        if live == 0 { let _ = std::fs::remove_file(path); }
    }
}
'''+s[b:]
def window(s):
 a=s.index('    pub async fn revoke_peer('); return s[:a]+s[a:].replace('        state.enabled = false;', '        // Mutation: leave pairing window open.', 1)
def notification(s):
 needle='    let notifications = runtime_notification_snapshot(since_notification_id);'
 return s.replace(needle, needle.replace('let notifications','let mut notifications')+'\n    notifications.entries = RUNTIME_NOTIFICATIONS.lock().unwrap().since(since_notification_id.unwrap_or(0));',1)
def inline_storage(s, marker):
 a=s.index(marker); b=s.index('}).await',a)
 region=s[a:b].replace('tokio::task::spawn_blocking(move || {','tokio::spawn(async move {').replace('.blocking_lock()', '.lock().await')
 return s[:a]+region+s[b:]
def inline_gc(s):
 a=s.index('pub(super) async fn run_transfer_maintenance_tick('); b=s.index('pub(super) async fn send_file_batch_to_peers(',a)
 region=s[a:b].replace('tokio::task::spawn_blocking(move || {','tokio::spawn(async move {').replace('database.blocking_lock()', 'database.lock().await')
 return s[:a]+region+s[b:]
def fanout(s): return s.replace('        let peer_batch = prepared.clone();','        let peer_batch = validate_prepared_batch_sources(prepared.prepared.clone()).await.unwrap();',1)
cases=[
 ('quota-fail-closed','shared/rust-core/src/db/storage.rs',lambda s:s.replace('            self.validate_live_payload_references()?;', ''),'quota_preflight_preserves_rows_when_live_payload_references_are_invalid','core'),
 ('r8-checkpoint-timeout','shared/rust-core/src/db/lifecycle.rs',lambda s:s.replace('conn.busy_timeout(std::time::Duration::ZERO)', 'conn.busy_timeout(std::time::Duration::from_secs(5))'),'default_delete_checkpoint_is_non_waiting_and_restores_busy_timeout','core'),
 ('r12-temp-gc','shared/rust-core/src/db/lifecycle.rs',lambda s:s.replace('                if !entry.file_type()?.is_file() {', '                if !entry.file_type()?.is_file() || is_atomic_temp {',1),'orphan_sweep_collects_owned_temps_and_preserves_live_writers','core'),
 ('r1-batch-rollback','shared/rust-core/src/db.rs',rollback,'batch_rollback_preserves_favorited_payloads_with_case_aliases','core'),
 ('r2-late-install','shared/rust-core/src/pairing/manager.rs',window,'revoke_peer_closes_window_before_a_late_session_is_installed','core'),
 ('r7-authenticated-route','macos/src-tauri/src/api/routes.rs',notification,'authenticated_runtime_route_keeps_notification_window_consistent_during_overflow','mac'),
 ('r8-receipt','shared/platform-network-server.rs',lambda s:inline_storage(s,'    let receipt = tokio::task::spawn_blocking'),'production_admission_and_gc_run_storage_work_off_the_async_thread','mac'),
 ('r8-quota','shared/platform-network-server.rs',lambda s:inline_storage(s,'        let preflight = tokio::task::spawn_blocking'),'production_admission_and_gc_run_storage_work_off_the_async_thread','mac'),
 ('r8-gc','shared/platform-clipboard-transfer.rs',inline_gc,'production_admission_and_gc_run_storage_work_off_the_async_thread','mac'),
 ('r11-long-poll','macos/src-tauri/src/api/transport.rs',lambda s:s.replace('"get_history" | "get_preview_data" | "wait_runtime_snapshot"','"get_history" | "get_preview_data"'),'authenticated_long_poll_disconnect_returns_connection_permit','mac'),
 ('r11-fanout','shared/platform-clipboard-transfer.rs',fanout,'production_fanout_and_resume_reuse_source_hashes_and_check_completion','mac'),
]
for name, file, mutate, test, kind in cases:
 path=root/file; original=path.read_bytes(); changed=mutate(original.decode()).encode(); assert original!=changed, name
 snapshots=evidence/'mutation-source-snapshots';snapshots.mkdir(exist_ok=True)
 (snapshots/(name+'-source.rs.txt')).write_bytes(original);(snapshots/(name+'-mutation.rs.txt')).write_bytes(changed)
 env['CARGO_TARGET_DIR']=str(Path('/Users/monet/TailSync/TailSync-remediation')/('target' if kind=='core' else 'macos/src-tauri/target'))
 cmd=['cargo','test','--locked','--manifest-path',('shared/rust-core/Cargo.toml' if kind=='core' else 'macos/src-tauri/Cargo.toml'),'--lib',test,'--','--test-threads=1','--nocapture']
 try:
  path.write_bytes(changed)
  red=subprocess.run(cmd,cwd=root,env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=180)
  (evidence/(name+'-mutation-red.log')).write_bytes(red.stdout)
 finally: path.write_bytes(original)
 assert red.returncode!=0 and b'test result: FAILED' in red.stdout,(name,red.stdout[-2000:])
 green=subprocess.run(cmd,cwd=root,env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=180)
 (evidence/(name+'-mutation-green.log')).write_bytes(green.stdout)
 assert green.returncode==0 and b'1 passed' in green.stdout,(name,green.stdout[-2000:])
 results.append(dict(name=name,file=file,test=test,command=cmd,source_sha256=hashlib.sha256(original).hexdigest(),mutation_sha256=hashlib.sha256(changed).hexdigest(),red_exit=red.returncode,green_exit=green.returncode))
 (evidence/'mutations.json').write_text(json.dumps(results,indent=2)+'\n')
 print(name,'executed red -> green',flush=True)

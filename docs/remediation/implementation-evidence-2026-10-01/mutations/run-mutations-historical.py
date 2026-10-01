from pathlib import Path
import subprocess, os, hashlib, json
root=Path('/private/tmp/tailsync-implementation.BzV6PH/mutation-src')
evidence=root.parent/'mutations'; evidence.mkdir(exist_ok=True)
env=os.environ.copy(); env.update(TAILSYNC_DATA_DIR=str(root.parent/'data'),TAILSYNC_V1_DATA_DIR=str(root.parent/'v1'))
cases=[]
def change_lane(s):
 a=s.index('        match command {',s.index('impl DeliveryLane'));b=s.index('\n    }',a);return s[:a]+'        let _ = command; Self::Events'+s[b:]
def change_hash(s): return s.replace('        let peer_batch = prepared.clone();','        let peer_batch = validate_prepared_batch_sources(prepared.prepared.clone()).await.unwrap();',1)
def change_pending(s):
 a=s.index('        if store\n            .get(hostname)') if '        if store\n            .get(hostname)' in s else s.index('        if store.get(hostname)');b=s.index('        store\n            .upsert(record)',a);return s[:a]+s[b:]
def change_identity(s):
 a=s.index('    pub(super) fn remove_unreferenced_payload_paths');b=s.index('    pub fn sweep_orphan_payloads',a)
 part=s[a:b].replace('                    Ok(identity) if live.contains(&identity) => continue,','').replace('                Ok(identity) if live.contains(&identity) => continue,','')
 return s[:a]+part+s[b:]
cases=[
 ('R1-unlink-identity','shared/rust-core/src/db/lifecycle.rs',change_identity,'every_history_cleanup_preserves_favorite_payload_case_aliases','core'),
 ('R5-worker-lane','shared/rust-core/src/peer/pool.rs',change_lane,'s2_f5_file_ack_does_not_block_text','core'),
 ('R9-recovery-key','shared/rust-core/src/pairing/manager.rs',change_pending,'pending_recovery_requires_the_same_authenticated_key_and_is_never_active','core'),
 ('R10-unsupported-ipv6','shared/rust-core/src/peer/directory.rs',lambda s:s.replace('            IpAddr::V4(ip) if ip.is_private() => Some(0),','            IpAddr::V4(ip) if ip.is_private() => Some(0),\n            IpAddr::V6(ip) if (ip.segments()[0] & 0xfe00) == 0xfc00 => Some(1),',1),'local_lan_ip_prefers_private_ipv4_and_never_returns_unspecified','core'),
 ('R11-incoming-callsite','shared/rust-core/src/sync/batches.rs',lambda s:s.replace('crate::private_fs::create_private_dir_all(incoming_dir)','std::fs::create_dir_all(incoming_dir)'),'actual_incoming_batch_creates_private_directories_and_manifest','core'),
 ('R12-temp-exemption','shared/rust-core/src/db/lifecycle.rs',lambda s:s.replace('                if !entry.file_type()?.is_file() {','                if !entry.file_type()?.is_file() || is_atomic_temp {',1),'orphan_sweep_collects_owned_temps_and_preserves_live_writers','core'),
 ('R11-api-route','macos/src-tauri/src/api/transport.rs',lambda s:s.replace('"get_history" | "get_preview_data" | "wait_runtime_snapshot"','"get_history" | "get_preview_data"'),'authenticated_long_poll_disconnect_returns_connection_permit','mac'),
 ('R11-production-fanout','shared/platform-clipboard-transfer.rs',change_hash,'production_fanout_and_resume_reuse_source_hashes_and_check_completion','mac'),
]
results=[]
for name, file, mutate, test, kind in cases:
 path=root/file; original=path.read_bytes(); changed=mutate(original.decode()).encode()
 assert original != changed, (name,'mutation did not match')
 target='/Users/monet/TailSync/TailSync-remediation/target' if kind=='core' else '/Users/monet/TailSync/TailSync-remediation/macos/src-tauri/target'
 env['CARGO_TARGET_DIR']=target
 command=['cargo','test','--locked']+(['-p','tailsync-core'] if kind=='core' else ['--manifest-path','macos/src-tauri/Cargo.toml','--lib'])+[test,'--','--test-threads=1']
 try:
  path.write_bytes(changed)
  red=subprocess.run(command,cwd=root,env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=180)
  (evidence/(name+'-red.log')).write_bytes(red.stdout)
 finally: path.write_bytes(original)
 assert red.returncode != 0 and b'test result: FAILED' in red.stdout, (name,'mutation did not produce a failing executed test',red.stdout[-1500:])
 green=subprocess.run(command,cwd=root,env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=180)
 (evidence/(name+'-green.log')).write_bytes(green.stdout)
 assert green.returncode == 0 and b'1 passed' in green.stdout, (name,green.stdout[-1500:])
 record=dict(issue=name,file=file,test=test,source_sha256=hashlib.sha256(original).hexdigest(),mutation_sha256=hashlib.sha256(changed).hexdigest(),red_exit=red.returncode,green_exit=green.returncode)
 results.append(record);(evidence/'results.json').write_text(json.dumps(results,ensure_ascii=False,indent=2)+'\n')
 print(name,'red -> green',flush=True)

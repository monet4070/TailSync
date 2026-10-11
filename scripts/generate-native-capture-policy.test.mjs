import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
const generator = resolve('scripts/generate-native-capture-policy.mjs');
function fixture(callback) {
 const root=mkdtempSync(resolve(tmpdir(),'tailsync-policy-'));
 try {
  for (const dir of ['shared/schema','shared/tailsync-protocol/src','macos/src-tauri']) mkdirSync(resolve(root,dir),{recursive:true});
  const policy=JSON.parse(readFileSync('shared/schema/native-capture-policy.json','utf8'));
  writeFileSync(resolve(root,'shared/schema/native-capture-policy.json'),JSON.stringify(policy));
  writeFileSync(resolve(root,'macos/src-tauri/clipboard-helper.swift'),'// BEGIN GENERATED NATIVE POLICY\n// END GENERATED NATIVE POLICY\n');
  callback(root,policy,(extra=[])=>spawnSync(process.execPath,[generator,'--root',root,...extra],{encoding:'utf8'}));
 } finally {rmSync(root,{recursive:true,force:true});}
}
test('generated Rust and Swift agree; independent drift on either side fails',()=>fixture((root,policy,run)=>{
 assert.equal(run().status,0);assert.equal(run(['--check']).status,0);
 for (const file of ['shared/tailsync-protocol/src/native_policy.generated.rs','macos/src-tauri/clipboard-helper.swift']) {
  const path=resolve(root,file),saved=readFileSync(path,'utf8');writeFileSync(path,saved.replace('4194304','4194305'));
  assert.notEqual(run(['--check']).status,0);writeFileSync(path,saved);
 }
}));
test('overlapping suppression and byte admission is rejected',()=>fixture((root,policy,run)=>{
 policy.common.push([policy.suppressed[0],100]);writeFileSync(resolve(root,'shared/schema/native-capture-policy.json'),JSON.stringify(policy));
 assert.notEqual(run().status,0);
}));
test('invalid limits and type names are rejected',()=>fixture((root,policy,run)=>{
 for(const invalid of [0,33554433,1.5]) {
  policy.opaque_limit=invalid;writeFileSync(resolve(root,'shared/schema/native-capture-policy.json'),JSON.stringify(policy));assert.notEqual(run().status,0);
 }
 policy.opaque_limit=4194304;policy.deferred.push('bad\nname');writeFileSync(resolve(root,'shared/schema/native-capture-policy.json'),JSON.stringify(policy));assert.notEqual(run().status,0);
}));
test('image format list and cover budget drift on either side fails',()=>fixture((root,policy,run)=>{
 assert.equal(run().status,0);assert.equal(run(['--check']).status,0);
 const cases={
  'shared/tailsync-protocol/src/native_policy.generated.rs':[
   ['("public.png", "image/png")','("public.png", "image/jpeg")'],
   ['COVER_FRAME_LIMIT: u32 = 20','COVER_FRAME_LIMIT: u32 = 21'],
   ['COVER_PIXEL_LIMIT: u64 = 8388608','COVER_PIXEL_LIMIT: u64 = 8388609'],
  ],
  'macos/src-tauri/clipboard-helper.swift':[
   ['let policyImageFormats: Set<String> = ["public.png"','let policyImageFormats: Set<String> = ["public.tiff"'],
   ['let policyCoverFrameLimit = 20','let policyCoverFrameLimit = 21'],
   ['let policyCoverPixelLimit = 8388608','let policyCoverPixelLimit = 8388609'],
  ],
 };
 for (const [file,tampers] of Object.entries(cases)) {
  const path=resolve(root,file),saved=readFileSync(path,'utf8');
  for (const [from,to] of tampers) {
   assert.ok(saved.includes(from),`expected generated marker in ${file}: ${from}`);
   writeFileSync(path,saved.replace(from,to));
   assert.notEqual(run(['--check']).status,0,`drift not detected in ${file}: ${from}`);
  }
  writeFileSync(path,saved);
 }
}));
test('illegal image formats and cover budgets are rejected',()=>fixture((root,policy,run)=>{
 const write=()=>writeFileSync(resolve(root,'shared/schema/native-capture-policy.json'),JSON.stringify(policy));
 for(const invalid of [0,-1,1.5,21,1000,null,'20']) { policy.cover_budget.frame_limit=invalid;write();assert.notEqual(run().status,0,`frame_limit ${invalid}`); }
 policy.cover_budget.frame_limit=20;
 for(const invalid of [0,-1,1.5,8*1024*1024+1,32*1024*1024,null,'8388608']) { policy.cover_budget.pixel_limit=invalid;write();assert.notEqual(run().status,0,`pixel_limit ${invalid}`); }
 policy.cover_budget.pixel_limit=8388608;
 for (const image_formats of [
  [],
  [['public.png','image/png'],['public.png','image/jpeg']],
  [['public.png','not a mime']],
  [['public.png','text/html']],
  [['public.png','application/png']],
  [['bad\nname','image/png']],
  [['com.example.not-common','image/png']],
 ]) { policy.image_formats=image_formats;write();assert.notEqual(run().status,0,`image_formats ${JSON.stringify(image_formats)}`); }
}));

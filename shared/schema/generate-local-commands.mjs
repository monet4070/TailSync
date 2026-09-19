import { readFileSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const manifest = JSON.parse(readFileSync(resolve(root,'shared/schema/local-commands.json'),'utf8'));
const title = name => name.split('_').map(word => word[0].toUpperCase()+word.slice(1)).join('');
const write = (file, content) => {
  const path = resolve(root,file);
  if (process.argv.includes('--check')) {
    if (readFileSync(path,'utf8').replaceAll('\r\n','\n') !== content) throw new Error(`${file} is stale`);
  } else writeFileSync(path,content);
};
for (const [platform, surfaces] of Object.entries(manifest)) {
  const seen = new Set();
  let rust = '// Generated from shared/schema/local-commands.json. Do not edit.\n';
  for (const [module,names] of Object.entries(surfaces.json)) {
    for (const name of names) { if (seen.has(name)) throw new Error(`Duplicate command ${name}`); seen.add(name); }
    rust += `#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub(super) enum ${title(module)}Command { ${names.map(title).join(',')} }\n`;
  }
  rust += `#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub(super) enum LocalCommand { ${Object.keys(surfaces.json).map(m => `${title(m)}(${title(m)}Command)`).join(',')}${surfaces.binary.length ? ', BinaryPreview' : ''} }\n`;
  rust += 'impl LocalCommand { pub(super) fn parse(name: &str) -> Option<Self> { Some(match name {\n';
  for (const [module,names] of Object.entries(surfaces.json)) for (const name of names) {
    rust += `${JSON.stringify(name)} => Self::${title(module)}(${title(module)}Command::${title(name)}),\n`;
  }
  for (const name of surfaces.binary) rust += `${JSON.stringify(name)} => Self::BinaryPreview,\n`;
  rust += '_ => return None, }) } }\n';
  rust += '#[cfg(test)] mod tests { use super::*; #[test] fn all_registered_commands_parse_and_unknown_is_rejected() {\n';
  for (const name of [...seen, ...surfaces.binary]) rust += `assert!(LocalCommand::parse(${JSON.stringify(name)}).is_some());\n`;
  rust += 'assert!(LocalCommand::parse("unknown_command").is_none()); } }\n';
  const formatted = spawnSync('rustfmt',['--edition','2021','--emit','stdout'], { input: rust, encoding: 'utf8' });
  if (formatted.status !== 0) throw new Error(formatted.stderr);
  write(`${platform}/src-tauri/src/api/routes/registry.rs`,formatted.stdout);
  write(`${platform}/src-tauri/src/tauri-handler.generated.rs`,'// Generated from shared/schema/local-commands.json. Do not edit.\ntauri::generate_handler![\n'+surfaces.tauri.map(name => `    ${name},`).join('\n')+'\n]\n');
}
console.log('Generated exhaustive local dispatch and Tauri registration');

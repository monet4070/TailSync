import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
const template = await readFile(resolve(here, 'template.html'), 'utf8');
const design=JSON.parse(await readFile(resolve(here, '../Design/mac-ui-manifest.generated.json'),'utf8'));
const records = JSON.parse(await readFile(resolve(here, '../DemoData/history.json'), 'utf8'));
const landscape = await readFile(resolve(here, '../TailSync.Native/Assets/Landscape.svg'), 'utf8');
const fragment = template
  .replace('__MAC_DESIGN__',JSON.stringify(design).replaceAll('<','\\u003c'))
  .replace('__HISTORY_DATA__', JSON.stringify(records).replaceAll('<', '\\u003c'))
  .replace('__LANDSCAPE_DATA_URI__', `data:image/svg+xml;base64,${Buffer.from(landscape).toString('base64')}`);
if (fragment.includes('__HISTORY_DATA__') || fragment.includes('__LANDSCAPE_DATA_URI__')) throw new Error('Unresolved preview template');
if (Buffer.byteLength(fragment) > 1_000_000) throw new Error('Preview exceeds inline size budget');
await writeFile(resolve(here, 'tailsync-native.html'), fragment);
const inlineIndex = process.argv.indexOf('--inline-output');
if (inlineIndex >= 0) {
  const target = process.argv[inlineIndex + 1];
  if (!target) throw new Error('--inline-output requires a file path');
  await mkdir(dirname(resolve(target)), { recursive: true });
  await writeFile(resolve(target), fragment);
}
console.log(`Prepared ${records.length} demo entries; ${Buffer.byteLength(fragment)} byte preview.`);

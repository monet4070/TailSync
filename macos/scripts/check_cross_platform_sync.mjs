import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const canonicalScript = resolve(
  repositoryRoot,
  'windows/scripts/check_cross_platform_sync.mjs',
);
const result = spawnSync(process.execPath, [canonicalScript, ...process.argv.slice(2)], {
  stdio: 'inherit',
});

if (result.error) throw result.error;
process.exit(result.status ?? 1);

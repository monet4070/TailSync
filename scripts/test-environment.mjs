import { mkdtempSync, mkdirSync, chmodSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

// Override inherited app paths: a developer's normal environment may point
// at live data. Each invocation owns its directory and can remove only that.
export function createIsolatedTestEnvironment(env = process.env) {
  const root = mkdtempSync(join(tmpdir(), 'tailsync-tests-'));
  try {
    chmodSync(root, 0o700);
    const paths = {
      TAILSYNC_DATA_DIR: join(root, 'data'),
      TAILSYNC_STORAGE_DIR: join(root, 'storage'),
      TAILSYNC_V1_DATA_DIR: join(root, 'legacy'),
    };
    for (const path of Object.values(paths)) mkdirSync(path, { mode: 0o700 });
    return {
      root,
      env: { ...env, ...paths },
      cleanup: () => rmSync(root, { recursive: true, force: true, maxRetries: 3, retryDelay: 100 }),
    };
  } catch (error) {
    rmSync(root, { recursive: true, force: true });
    throw error;
  }
}

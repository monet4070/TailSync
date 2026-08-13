import assert from 'node:assert/strict';
import test from 'node:test';

import { releaseChannel, validateVersions } from './validate-release-version.mjs';

test('accepts a tag matching every application manifest', () => {
  assert.equal(validateVersions('v2.1.0', {
    windows: '2.1.0',
    macos: '2.1.0',
    shared: '2.1.0',
  }), '2.1.0');
});

test('rejects a release when any application manifest drifts', () => {
  assert.throws(
    () => validateVersions('v2.2.0', { windows: '2.2.0', macos: '2.1.0' }),
    /macos=2\.1\.0/,
  );
});

test('classifies stable and prerelease tags without channel ambiguity', () => {
  assert.equal(releaseChannel('v2.2.0'), 'stable');
  assert.equal(releaseChannel('v2.2.0-rc.1'), 'prerelease');
  assert.throws(() => releaseChannel('release-2.2.0'), /semantic version/);
});

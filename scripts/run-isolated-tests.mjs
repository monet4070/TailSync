#!/usr/bin/env node
import { spawn } from 'node:child_process';
import { constants } from 'node:os';
import { createIsolatedTestEnvironment } from './test-environment.mjs';

const command = process.argv.slice(2);
if (command[0] === '--') command.shift();
if (!command.length) throw new Error('Usage: run-isolated-tests.mjs -- <command> [arguments...]');

const environment = createIsolatedTestEnvironment();
try {
  const child = spawn(command[0], command.slice(1), { env: environment.env, stdio: 'inherit' });
  const signals = ['SIGINT', 'SIGTERM'];
  const forward = (signal) => child.kill(signal);
  const handlers = signals.map((signal) => () => forward(signal));
  signals.forEach((signal, index) => process.on(signal, handlers[index]));
  try {
    process.exitCode = await new Promise((accept, reject) => {
      child.once('error', reject);
      child.once('close', (code, signal) => accept(code ?? (128 + (constants.signals[signal] ?? 1))));
    });
  } finally {
    signals.forEach((signal, index) => process.removeListener(signal, handlers[index]));
  }
} finally {
  environment.cleanup();
}

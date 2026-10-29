import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
const source = readFileSync(new URL('./guide.json', import.meta.url), 'utf8');
const guide = JSON.parse(source);
if (new Set(guide.steps.map(step => step.id)).size !== guide.steps.length) throw new Error('Duplicate onboarding step');
for (const step of guide.steps) {
  if (!['connections', 'history', 'settings'].includes(step.window) || !step.target) throw new Error('Unknown tour target');
}
const output = '// Generated from shared/onboarding/guide.json. Do not edit.\nimport Foundation\n\nenum OnboardingContent {\n    static let json = #"""\n' + source.trim() + '\n"""#\n}\n';
const target = new URL('../../macos/swift-ui/Sources/TailSync/Models/OnboardingContent.generated.swift', import.meta.url);
if (process.argv.includes('--check')) {
  if (readFileSync(target, 'utf8') !== output) throw new Error(`${fileURLToPath(target)} is stale`);
} else writeFileSync(target, output);
const demo = readFileSync(new URL('./demo.json', import.meta.url), 'utf8');
const demoOutput = '// Generated from shared/onboarding/demo.json. Do not edit.\nimport Foundation\n\nenum OnboardingDemoContent {\n    static let json = #"""\n' + demo.trim() + '\n"""#\n}\n';
const demoTarget = new URL('../../macos/swift-ui/Sources/TailSync/Models/OnboardingDemo.generated.swift', import.meta.url);
if (process.argv.includes('--check')) {
  if (readFileSync(demoTarget, 'utf8') !== demoOutput) throw new Error(`${fileURLToPath(demoTarget)} is stale`);
} else writeFileSync(demoTarget, demoOutput);
console.log('Onboarding content is in sync.');

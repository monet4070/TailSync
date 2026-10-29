import { build } from 'esbuild';
import { createHash } from 'node:crypto';
import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

const root = fileURLToPath(new URL('../../', import.meta.url));
const output = resolve(root, 'macos/swift-ui/Sources/TailSync/Resources/Markdown');
const bundle = await build({ stdin: { contents: `
import { Marked } from 'marked';
import katex from 'katex';
import DOMPurify from 'dompurify';
import {createMarkdownParser} from '../shared/markdown-preview/parser.mjs';
import {installNativeMarkdown} from '../shared/markdown-preview/native-runtime.mjs';
const parse = createMarkdownParser((extensions,walkTokens,renderer) => {
 const parser = new Marked({gfm:true,breaks:false,extensions,walkTokens,renderer});
 return source => parser.parse(source,{async:false});
}, (source,displayMode) => katex.renderToString(source,{
 displayMode,output:'htmlAndMathml',throwOnError:true,trust:false,strict:'ignore',maxExpand:200,maxSize:12
}));
installNativeMarkdown(parse,DOMPurify);
`, resolveDir: resolve(root, 'windows') }, bundle: true, write: false, minify: true,
  format: 'iife', platform: 'browser', target: 'safari16', legalComments: 'inline' });

let css = await readFile(resolve(root, 'windows/node_modules/katex/dist/katex.min.css'), 'utf8');
// macOS needs no server, file URL permission, or font network request.
const fontURLs = [...new Set([...css.matchAll(/url\(([^)]+\.woff2)\)/g)].map(match => match[1]))];
for (const font of fontURLs) {
  const data = await readFile(resolve(root, 'windows/node_modules/katex/dist', font));
  css = css.replaceAll(`url(${font})`, `url(data:font/woff2;base64,${data.toString('base64')})`);
}
css = css.replace(/,url\([^)]+\.(?:woff|ttf)\) format\("(?:woff|truetype)"\)/g, '');
css += '\n' + await readFile(resolve(root, 'shared/markdown-preview/article.css'), 'utf8');
const licenses = await Promise.all(['marked/LICENSE','dompurify/LICENSE','katex/LICENSE']
  .map(async file => `\n${file}\n${await readFile(resolve(root, 'windows/node_modules', file), 'utf8')}`));
const files = { 'engine.js': bundle.outputFiles[0].text, 'preview.css': css, 'LICENSES.txt': licenses.join('\n') };
const sourceFiles = ['shared/markdown-preview/parser.mjs','shared/markdown-preview/dom.mjs',
  'shared/markdown-preview/native-runtime.mjs','shared/markdown-preview/article.css',
  'windows/scripts/build-markdown-assets.mjs','windows/package-lock.json'];
const digest = value => createHash('sha256').update(value).digest('hex');
const manifest = { sources: {}, outputs: {} };
for (const file of sourceFiles) manifest.sources[file] = digest(await readFile(resolve(root, file)));
for (const [name, contents] of Object.entries(files)) manifest.outputs[name] = digest(contents);
files['manifest.json'] = JSON.stringify(manifest, null, 2) + '\n';
if (process.argv.includes('--check')) {
  for (const [name, contents] of Object.entries(files)) {
    if (await readFile(resolve(output, name), 'utf8') !== contents) throw new Error(`Stale Markdown asset: ${name}`);
  }
  console.log('Offline Markdown assets match sources and dependency lockfile.');
} else {
  await mkdir(output, { recursive: true });
  for (const [name, contents] of Object.entries(files)) await writeFile(resolve(output, name), contents);
  console.log(`Built offline Markdown assets (${bundle.outputFiles[0].contents.length} JavaScript bytes).`);
}

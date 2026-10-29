import { Marked, type TokenizerAndRendererExtension, type Token, type RendererObject } from 'marked';
import katex from 'katex';
import { createMarkdownParser } from '../../../../shared/markdown-preview/parser.mjs';

export const parseMarkdown = createMarkdownParser((extensions, walkTokens, renderer) => {
  const parser = new Marked({ gfm: true, breaks: false,
    extensions: extensions as TokenizerAndRendererExtension[], renderer: renderer as RendererObject,
    walkTokens: (token: Token) => walkTokens(token) });
  return source => parser.parse(source, { async: false });
}, (source, displayMode) => katex.renderToString(source, {
  displayMode, output: 'htmlAndMathml', throwOnError: true,
  trust: false, strict: 'ignore', maxExpand: 200, maxSize: 12,
}));

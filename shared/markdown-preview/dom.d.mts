import type { MarkdownResult } from './parser.mjs';
export function explicitWebUrl(value: string): string | null;
export function prepareMarkdownDOM(result: MarkdownResult, sanitize: (html: string, config: object) => DocumentFragment, document: Document): DocumentFragment | null;

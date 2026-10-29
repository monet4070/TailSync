import { vi } from 'vitest';
import { parseMarkdown } from '../preview/renderers/markdownParser';

/** Executes the production worker parser; lifecycle tests can hold/reorder replies. */
export class MarkdownTestWorker {
  static instances: MarkdownTestWorker[] = [];
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: (() => void) | null = null;
  terminated = false;
  source = '';
  static autoReply = true;
  constructor() { MarkdownTestWorker.instances.push(this); }
  postMessage(source: string) {
    this.source = source;
    if (MarkdownTestWorker.autoReply) queueMicrotask(() => this.reply());
  }
  reply() { this.onmessage?.({ data: parseMarkdown(this.source) } as MessageEvent); }
  terminate() { this.terminated = true; }
}

export function installMarkdownTestWorker() {
  MarkdownTestWorker.instances = []; MarkdownTestWorker.autoReply = true;
  vi.stubGlobal('Worker', MarkdownTestWorker);
}

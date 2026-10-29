import { parseMarkdown } from './markdownParser';

self.onmessage = (event: MessageEvent<string>) => {
  self.postMessage(parseMarkdown(event.data));
};

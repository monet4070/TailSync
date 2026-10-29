import { prepareMarkdownDOM } from './dom.mjs';

export function installNativeMarkdown(parse, purifier) {
  let renders = 0;
  function setAppearance(appearance) {
    for (const [key, value] of Object.entries(appearance.colors)) document.documentElement.style.setProperty(key, value);
    document.documentElement.lang = appearance.language;
    document.documentElement.style.colorScheme = appearance.dark ? 'dark' : 'light';
    document.getElementById('article').style.fontSize = `${appearance.fontSize}px`;
    document.body.style.background = appearance.colors['--md-background'];
  }
  function render(source, appearance, sourceTruncated) {
    const start = performance.now();
    setAppearance(appearance);
    const result = parse(source);
    const article = document.getElementById('article');
    const fragment = prepareMarkdownDOM(result,
      (html, config) => purifier.sanitize(html, config), document);
    if (fragment) article.replaceChildren(fragment);
    else { const pre = document.createElement('pre'); pre.className = 'md-source-fallback'; pre.textContent = source; article.replaceChildren(pre); }
    const truncated = sourceTruncated || result.truncated || !fragment;
    const notice = document.getElementById('notice');
    notice.hidden = !truncated; notice.textContent = truncated ? appearance.truncatedMessage : '';
    return { renders: ++renders, truncated, durationMs: performance.now() - start };
  }
  globalThis.TailSyncMarkdown = Object.freeze({ render, setAppearance });
}

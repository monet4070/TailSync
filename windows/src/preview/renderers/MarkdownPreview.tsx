import { useCallback, useLayoutEffect, useMemo, useRef, useState } from 'react';
import DOMPurify from 'dompurify';
import { open } from '@tauri-apps/plugin-shell';
import { explicitWebUrl, prepareMarkdownDOM } from '../../../../shared/markdown-preview/dom.mjs';
import { limitMarkdownSource, type MarkdownResult } from '../../../../shared/markdown-preview/parser.mjs';
import 'katex/dist/katex.min.css';
import '../../../../shared/markdown-preview/article.css';
import { useModifierWheelZoom, usePreviewTextFontSize, zoomFromWheel } from '../previewPreferences';

export function MarkdownPreview({ data, t }: { data: Uint8Array; t?: (key: string) => string }) {
  const [fontSize, setFontSize] = usePreviewTextFontSize();
  const scrollRef = useModifierWheelZoom<HTMLDivElement>((deltaY) => {
    setFontSize(value => zoomFromWheel(value, deltaY, 12, 32));
  });
  const articleRef = useRef<HTMLElement>(null);
  const source = useMemo(() => limitMarkdownSource(new TextDecoder('utf-8').decode(data)), [data]);
  const [state, setState] = useState({ truncated: false, loading: true });

  useLayoutEffect(() => {
    const article = articleRef.current;
    if (!article) return;
    let disposed = false;
    let worker: Worker | undefined;
    const showSource = () => {
      if (disposed) return;
      const pre = document.createElement('pre');
      pre.className = 'md-source-fallback'; pre.textContent = source.source;
      article.replaceChildren(pre); setState({ truncated: true, loading: false });
    };
    article.replaceChildren(); setState({ truncated: source.truncated, loading: true });
    const timeout = window.setTimeout(() => { worker?.terminate(); showSource(); }, 2500);
    try {
      worker = new Worker(new URL('./markdown.worker.ts', import.meta.url), { type: 'module' });
      worker.onmessage = (event: MessageEvent<MarkdownResult>) => {
        if (disposed) return;
        window.clearTimeout(timeout);
        const fragment = prepareMarkdownDOM(event.data,
          (html, config) => DOMPurify.sanitize(html, { ...config, RETURN_DOM_FRAGMENT: true }), document);
        if (fragment) {
          article.replaceChildren(fragment);
          setState({ truncated: source.truncated || event.data.truncated, loading: false });
        } else showSource();
        worker?.terminate();
      };
      worker.onerror = () => { window.clearTimeout(timeout); worker?.terminate(); showSource(); };
      worker.postMessage(source.source);
    } catch { window.clearTimeout(timeout); showSource(); }
    return () => { disposed = true; window.clearTimeout(timeout); worker?.terminate(); };
  }, [source]);

  const openLink = useCallback((event: React.MouseEvent<HTMLElement>) => {
    const anchor = event.target instanceof Element ? event.target.closest<HTMLAnchorElement>('a[href]') : null;
    if (!anchor) return;
    event.preventDefault();
    const url = explicitWebUrl(anchor.getAttribute('href') ?? '');
    if (url) void open(url).catch(error => console.error('Could not open Markdown preview link:', error));
  }, []);

  return <div className="preview-markdown" ref={scrollRef}>
    {state.loading && <div className="md-notice" role="status">{t?.('history.preview.loading') ?? 'Loading…'}</div>}
    {state.truncated && !state.loading && <div className="md-notice" role="status" data-testid="preview-truncated">
      {t?.('history.preview.truncated') ?? 'Preview limited; original content is unchanged.'}
    </div>}
    <article ref={articleRef} className="md-article" data-testid="preview-markdown"
      style={{ fontSize }} onClick={openLink} aria-busy={state.loading} />
  </div>;
}

// One bounded GFM/math policy for both desktop previews. No DOM or I/O here;
// Windows runs this in a disposable worker, macOS in WebKit's content process.
export const MARKDOWN_LIMITS = Object.freeze({ source: 256 * 1024, tokens: 12000,
  tableRows: 500, tableColumns: 32, formulas: 128, formulaLength: 2048,
  output: 2 * 1024 * 1024, mathOutput: 1024 * 1024 });

export function escapeHTML(value) {
  return value.replace(/[&<>"']/g, character => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[character]);
}

export function limitMarkdownSource(source) {
  if (source.length <= MARKDOWN_LIMITS.source) return { source, truncated: false };
  const boundary = source.lastIndexOf('\n', MARKDOWN_LIMITS.source);
  return { source: source.slice(0, boundary > MARKDOWN_LIMITS.source * .75 ? boundary : MARKDOWN_LIMITS.source), truncated: true };
}

export function createMarkdownParser(createParser, renderMath) {
  return function parseMarkdown(input) {
    const limited = limitMarkdownSource(input);
    const math = [];
    let truncated = limited.truncated;
    let tokens = 0;
    let mathBytes = 0;
    const mathToken = (raw, text, display) => ({ type: display ? 'mathBlock' : 'mathInline', raw, text, display });
    const extensions = [{
      name: 'mathBlock', level: 'block',
      start(source) { const match = /(?:^|\n) {0,3}(?:\$\$|\\\[)/.exec(source); return match?.index; },
      tokenizer(source) {
        const opening = /^ {0,3}(\$\$|\\\[)/.exec(source);
        if (!opening) return;
        const close = opening[1] === '$$' ? '$$' : '\\]';
        const end = source.indexOf(close, opening[0].length);
        if (end < 0 || !/^[ \t]*(?:\n|$)/.test(source.slice(end + close.length))) return;
        const raw = source.slice(0, end + close.length);
        return mathToken(raw, raw.slice(opening[0].length, -close.length).trim(), true);
      },
      renderer(token) { return placeholder(token); },
    }, {
      name: 'mathInline', level: 'inline',
      start(source) { const match = /\$|\\\(/.exec(source); return match?.index; },
      tokenizer(source) {
        const bracket = source.startsWith('\\(');
        if (!bracket && (!source.startsWith('$') || source.startsWith('$$') || /^\$\s/.test(source))) return;
        const opening = bracket ? 2 : 1;
        const close = bracket ? '\\)' : '$';
        // Do not mistake prices for formulas, or consume across paragraphs.
        let end = source.indexOf(close, opening);
        while (end > 0 && source[end - 1] === '\\') end = source.indexOf(close, end + close.length);
        if (end < opening || /[\n`]/.test(source.slice(opening, end)) ||
            (!bracket && (/\s/.test(source[end - 1]) || /\d/.test(source[end + 1] ?? '')))) return;
        return mathToken(source.slice(0, end + close.length), source.slice(opening, end), false);
      },
      renderer(token) { return placeholder(token); },
    }];
    function placeholder(token) {
      if (math.length >= MARKDOWN_LIMITS.formulas || token.text.length > MARKDOWN_LIMITS.formulaLength) {
        truncated = true;
        return `<code class="md-math-fallback">${escapeHTML(token.raw)}</code>`;
      }
      let html;
      try {
        html = renderMath(token.text, token.display);
        if (html.length > 128 * 1024 || html.length > MARKDOWN_LIMITS.mathOutput - mathBytes) {
          truncated = true;
          html = `<code class="md-math-fallback">${escapeHTML(token.raw)}</code>`;
        }
      } catch {
        html = `<code class="md-math-fallback">${escapeHTML(token.raw)}</code>`;
      }
      mathBytes += html.length;
      const index = math.push({ html, display: token.display }) - 1;
      const tag = token.display ? 'div' : 'span';
      return `<${tag} data-md-math="${index}">${escapeHTML(token.raw)}</${tag}>`;
    }
    try {
      const parse = createParser(extensions, token => {
        if (++tokens > MARKDOWN_LIMITS.tokens) throw new Error('Markdown token budget');
        if (token.type === 'table') {
          if (token.rows.length > MARKDOWN_LIMITS.tableRows || token.header.length > MARKDOWN_LIMITS.tableColumns) truncated = true;
          token.rows = token.rows.slice(0, MARKDOWN_LIMITS.tableRows).map(row => row.slice(0, MARKDOWN_LIMITS.tableColumns));
          token.header = token.header.slice(0, MARKDOWN_LIMITS.tableColumns);
          token.align = token.align.slice(0, MARKDOWN_LIMITS.tableColumns);
        }
      }, {
        image(token) { return `<span class="md-image-alt">${escapeHTML(token.text || '[image]')}</span>`; },
        checkbox(token) { return `<span class="md-task" role="img" aria-label="${token.checked ? 'Completed' : 'Incomplete'}">${token.checked ? '☑' : '☐'}</span> `; },
      });
      const html = parse(limited.source);
      if (html.length + mathBytes > MARKDOWN_LIMITS.output) throw new Error('Markdown output budget');
      return { html, math, truncated, plain: false };
    } catch {
      return { html: `<pre class="md-source-fallback">${escapeHTML(limited.source)}</pre>`, math: [], truncated: true, plain: true };
    }
  };
}

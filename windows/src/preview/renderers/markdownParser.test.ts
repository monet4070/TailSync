import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import DOMPurify from 'dompurify';
import { parseMarkdown } from './markdownParser';
import { prepareMarkdownDOM } from '../../../../shared/markdown-preview/dom.mjs';
import { MARKDOWN_LIMITS } from '../../../../shared/markdown-preview/parser.mjs';

function fragment(source: string) {
  const result = parseMarkdown(source);
  const dom = prepareMarkdownDOM(result, (html, config) => DOMPurify.sanitize(html, { ...config, RETURN_DOM_FRAGMENT: true }), document);
  return { result, dom: dom! };
}

describe('shared desktop Markdown policy', () => {
  it('preserves table columns, alignment, escaped pipes and nested block structure', () => {
    const source = readFileSync(resolve(process.cwd(), '../shared/markdown-preview/acceptance.md'), 'utf8');
    const { dom, result } = fragment(source);
    expect(result.truncated).toBe(false);
    expect(dom.querySelectorAll('tbody tr')).toHaveLength(3);
    expect([...dom.querySelectorAll('tbody tr')].map(row => row.children.length)).toEqual([4, 4, 4]);
    expect(dom.querySelector('tbody tr:nth-child(2) td:nth-child(2)')?.textContent).toContain('a|b');
    expect(dom.querySelector('th:nth-child(3)')?.getAttribute('align')).toBe('right');
    expect(dom.querySelector('.md-table-scroll')?.getAttribute('tabindex')).toBe('0');
    expect(dom.querySelectorAll('ol > li')).toHaveLength(2);
    expect(dom.querySelectorAll('ol ul > li')).toHaveLength(2);
    expect(dom.querySelectorAll('blockquote p')).toHaveLength(2);
    expect(dom.querySelectorAll('.md-math-display')).toHaveLength(2);
    expect(dom.querySelectorAll('.katex').length).toBeGreaterThan(6);
    expect(dom.querySelector('pre code')?.textContent).toContain('$x^2$');
    expect(dom.textContent).toContain('价格 $5 和 $10');
  });

  it('keeps prices, escaped delimiters, inline/fenced code, and invalid formulas readable', () => {
    const { dom } = fragment('Price $5 and $10. Code `$x$`. \\$cash. Math \\(a+b\\).\n\n```tex\n$$x$$\n```\n\n$\\notACommand{x}$');
    expect(dom.textContent).toContain('Price $5 and $10. Code $x$. $cash.');
    expect(dom.querySelectorAll('.katex')).toHaveLength(1);
    expect(dom.querySelector('pre code')?.textContent).toContain('$$x$$');
    expect(dom.querySelector('.md-math-fallback')?.textContent).toBe('$\\notACommand{x}$');
  });

  it('never restores active HTML or resource URLs through the formula exception', () => {
    const { dom } = fragment('<script>alert(1)</script><img src="https://bad.test/x"><div style="background:url(https://bad.test)">text</div>\n\n[bad](file:///secret)\n\n$\\href{javascript:alert(1)}{click}$\n\n$\\includegraphics{https://bad.test/x}$');
    expect(dom.querySelector('script,img,iframe,svg')).toBeNull();
    expect(dom.querySelector('[src],[onerror],[onclick]')).toBeNull();
    expect([...dom.querySelectorAll('a[href]')]).toEqual([]);
    expect(dom.querySelector('div[style]')).toBeNull();
    expect(dom.textContent).toContain('text');
  });

  it('bounds wide/long tables and formula expansion without discarding original data', () => {
    const source = '|A|B|\n|---|---|\n' + '|x|y|\n'.repeat(700);
    const { result, dom } = fragment(source);
    expect(result.truncated).toBe(true);
    expect(dom.querySelectorAll('tbody tr')).toHaveLength(MARKDOWN_LIMITS.tableRows);
    const formula = parseMarkdown('$x$ '.repeat(300));
    expect(formula.math).toHaveLength(MARKDOWN_LIMITS.formulas);
    expect(formula.truncated).toBe(true);
    const recursive = parseMarkdown('$$\\def\\a{\\a}\\a$$');
    expect(recursive.math[0].html).toContain('md-math-fallback');
    expect(source.split('\n')).toHaveLength(703);
  });
});

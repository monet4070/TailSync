export function explicitWebUrl(value) {
  try {
    const parsed = new URL(value);
    return parsed.protocol === 'http:' || parsed.protocol === 'https:' ? parsed.href : null;
  } catch { return null; }
}

export function prepareMarkdownDOM(result, sanitize, document) {
  const fragment = sanitize(result.html, {
    RETURN_DOM_FRAGMENT: true, USE_PROFILES: { html: true }, ALLOW_DATA_ATTR: false,
    ADD_ATTR: ['data-md-math'],
    FORBID_TAGS: ['audio','base','button','embed','form','iframe','img','input','link','meta','object','picture','source','style','svg','video'],
    FORBID_ATTR: ['formaction','ping','src','srcset','style','target','xlink:href'],
  });
  fragment.querySelectorAll('a[href]').forEach(anchor => {
    const safe = explicitWebUrl(anchor.getAttribute('href') ?? '');
    if (safe) { anchor.setAttribute('href', safe); anchor.setAttribute('rel', 'noopener noreferrer'); }
    else anchor.removeAttribute('href');
  });
  let installations = 0;
  fragment.querySelectorAll('[data-md-math]').forEach(node => {
    const formula = result.math[Number(node.getAttribute('data-md-math'))];
    node.removeAttribute('data-md-math');
    if (!formula || ++installations > 128) return;
    // Only locally generated KaTeX output crosses this second boundary. Raw
    // Markdown HTML was sanitized above and never receives a style exemption.
    node.innerHTML = formula.html;
    node.className = formula.display ? 'md-math md-math-display' : 'md-math';
  });
  fragment.querySelectorAll('table').forEach(table => {
    const wrapper = document.createElement('div');
    wrapper.className = 'md-table-scroll'; wrapper.tabIndex = 0;
    wrapper.setAttribute('role', 'region');
    wrapper.setAttribute('aria-label', document.documentElement.lang.startsWith('zh') ? '表格（可横向滚动）' : 'Table (scroll horizontally)');
    table.before(wrapper); wrapper.append(table);
  });
  // Includes formula expansion, rather than merely counting source tokens.
  if (fragment.querySelectorAll('*').length > 16000) return null;
  return fragment;
}

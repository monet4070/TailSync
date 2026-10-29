export const MARKDOWN_LIMITS: Readonly<{source: number; tokens: number; tableRows: number; tableColumns: number; formulas: number; formulaLength: number; output: number; mathOutput: number}>;
export interface MarkdownResult { html: string; math: { html: string; display: boolean }[]; truncated: boolean; plain: boolean }
export function escapeHTML(value: string): string;
export function limitMarkdownSource(source: string): {source: string; truncated: boolean};
export function createMarkdownParser(createParser: (extensions: unknown[], walk: (token: any) => void, renderer: object) => (source: string) => string,
  renderMath: (source: string, display: boolean) => string): (source: string) => MarkdownResult;

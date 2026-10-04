import { describe, expect, it } from 'vitest';
import { inline, renderMarkdown } from './markdown';
import guide from '../../../../USER_GUIDE.md?raw';

describe('renderMarkdown', () => {
  it('escapes HTML and renders inline marks', () => {
    expect(inline('a <b> & **bold** *it* `x<y`')).toBe('a &lt;b&gt; &amp; <strong>bold</strong> <em>it</em> <code>x&lt;y</code>');
  });
  it('renders headings, lists with continuation lines, tables, quotes, code and rules', () => {
    const md = '# T\n\nPara one\nwraps.\n\n- a\n  more\n- b\n\n| H1 | H2 |\n|---|---|\n| c | d |\n\n> quote\n> on\n\n```\ncode <x>\n```\n\n---\n';
    const html = renderMarkdown(md);
    expect(html).toContain('<h1>T</h1>');
    expect(html).toContain('<p>Para one wraps.</p>');
    expect(html).toContain('<ul><li>a more</li><li>b</li></ul>');
    expect(html).toContain('<thead><tr><th>H1</th><th>H2</th></tr></thead><tbody><tr><td>c</td><td>d</td></tr></tbody>');
    expect(html).toContain('<blockquote>quote on</blockquote>');
    expect(html).toContain('<pre><code>code &lt;x&gt;</code></pre>');
    expect(html).toContain('<hr>');
  });
  it('keeps the FAQ question on its own line', () => {
    expect(renderMarkdown('**Why?**\nBecause.')).toBe('<p><strong>Why?</strong><br>Because.</p>');
  });
  it('renders the bundled user guide with every section and no stray Markdown', () => {
    const html = renderMarkdown(guide);
    expect((html.match(/<h2>/g) ?? []).length).toBeGreaterThanOrEqual(12);
    expect(html).toContain('<table>');
    const text = html.replace(/<pre>[\s\S]*?<\/pre>/g, '').replace(/<[^>]+>/g, '');
    expect(text).not.toMatch(/\*\*|^#|\[CHECK/m);
  });
});

describe('inline marks around code', () => {
  it('bold may wrap a code span, and asterisks inside code stay literal', () => {
    expect(inline('**`name.csv`**, then `a*b*c`')).toBe('<strong><code>name.csv</code></strong>, then <code>a*b*c</code>');
  });
});

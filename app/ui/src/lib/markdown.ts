/** A small Markdown renderer for the bundled user guide (USER_GUIDE.md) only: headings, paragraphs, bullet lists,
 *  tables, block quotes, code blocks, rules and inline bold / italic / code. All text is HTML-escaped first; the
 *  input is the app's own bundled file, never user content. No dependency. */

const esc = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

export function inline(s: string): string {
  // Code spans are set aside first (their text is literal), so bold or italic may wrap them: **`name.csv`**.
  const code: string[] = [];
  const marked = esc(s)
    .replace(/`([^`]+)`/g, (_, c: string) => `\u0001${code.push(c) - 1}\u0001`)
    .replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>')
    .replace(/(^|[^*])\*([^*\s][^*]*?)\*(?!\*)/g, '$1<em>$2</em>');
  return marked.replace(/\u0001(\d+)\u0001/g, (_, i: string) => `<code>${code[Number(i)]}</code>`);
}

const cells = (row: string) =>
  row
    .trim()
    .replace(/^\|/, '')
    .replace(/\|$/, '')
    .split('|')
    .map((c) => c.trim());

export function renderMarkdown(md: string): string {
  const lines = md.replace(/\r\n/g, '\n').split('\n');
  const out: string[] = [];
  let i = 0;
  while (i < lines.length) {
    const l = lines[i];
    if (!l.trim()) {
      i++;
      continue;
    }
    if (l.startsWith('```')) {
      const body: string[] = [];
      i++;
      while (i < lines.length && !lines[i].startsWith('```')) body.push(lines[i++]);
      i++;
      out.push(`<pre><code>${esc(body.join('\n'))}</code></pre>`);
      continue;
    }
    const h = /^(#{1,4})\s+(.*)$/.exec(l);
    if (h) {
      out.push(`<h${h[1].length}>${inline(h[2])}</h${h[1].length}>`);
      i++;
      continue;
    }
    if (/^-{3,}\s*$/.test(l)) {
      out.push('<hr>');
      i++;
      continue;
    }
    if (l.trimStart().startsWith('|')) {
      const rows: string[] = [];
      while (i < lines.length && lines[i].trimStart().startsWith('|')) rows.push(lines[i++]);
      const body = rows.filter((r) => !/^\s*\|?[\s:|-]+\|?\s*$/.test(r));
      const [head, ...rest] = body;
      out.push(
        `<table><thead><tr>${cells(head).map((c) => `<th>${inline(c)}</th>`).join('')}</tr></thead><tbody>` +
          rest.map((r) => `<tr>${cells(r).map((c) => `<td>${inline(c)}</td>`).join('')}</tr>`).join('') +
          '</tbody></table>',
      );
      continue;
    }
    if (l.startsWith('>')) {
      const q: string[] = [];
      while (i < lines.length && lines[i].startsWith('>')) q.push(lines[i++].replace(/^>\s?/, ''));
      out.push(`<blockquote>${inline(q.join(' '))}</blockquote>`);
      continue;
    }
    if (/^- /.test(l)) {
      const items: string[] = [];
      while (i < lines.length && (/^- /.test(lines[i]) || (/^\s{2,}\S/.test(lines[i]) && items.length))) {
        if (/^- /.test(lines[i])) items.push(lines[i].slice(2).trim());
        else items[items.length - 1] += ' ' + lines[i].trim();
        i++;
      }
      out.push(`<ul>${items.map((it) => `<li>${inline(it)}</li>`).join('')}</ul>`);
      continue;
    }
    const para: string[] = [];
    while (
      i < lines.length &&
      lines[i].trim() &&
      !/^(#{1,4}\s|```|-{3,}\s*$|- |>|\s*\|)/.test(lines[i])
    )
      para.push(lines[i++].trim());
    if (!para.length) {
      out.push(`<p>${inline(l.trim())}</p>`); // a line no rule claimed: keep it as text
      i++;
      continue;
    }
    // "**Question?**" on its own line followed by an answer keeps its line break (the FAQ)
    // (inline marks may wrap across lines, so lines are joined before they are rendered)
    const faq = /^\*\*.*\*\*$/.test(para[0]) && para.length > 1;
    out.push(`<p>${faq ? `${inline(para[0])}<br>${inline(para.slice(1).join(' '))}` : inline(para.join(' '))}</p>`);
  }
  return out.join('\n');
}

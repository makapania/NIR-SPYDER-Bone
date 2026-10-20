import { expect, test } from '@playwright/test';

test('core screen renders a verdict, the scan list and the charts from the mock', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByTestId('verdict-word')).toHaveText('Good candidate');
  await expect(page.locator('.srow')).toHaveCount(23);
  await expect(page.locator('.uplot').first()).toBeVisible();
  await expect(page.getByText('Browser preview ok')).toBeVisible();
});

test("Matt's case: Borderline ≈ 1.2% with the ZooMS line and the positive-signs line", async ({ page }) => {
  await page.goto('/');
  await page.locator('[data-scan="Spectrum00039.asd"]').click();
  await expect(page.getByTestId('verdict-word')).toHaveText('Borderline');
  const card = page.locator('.verdict');
  await expect(card.locator('[data-note="zooms_better_good"]')).toContainText('Better chance with ZooMS');
  await expect(card.locator('[data-note="positive_signs_all_six"]')).toBeVisible();
});

test('one verdict for every analysis: no picker; the ZooMS line under the verdict, in the list and the details', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('group', { name: 'Analysis' })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'ZooMS', exact: true })).toHaveCount(0);
  // protein bands (m >= 0.3, two N-type bands): Unlikely stays Unlikely, the line says ZooMS has some chance
  await page.locator('[data-scan="Spectrum00034.asd"]').click();
  await expect(page.getByTestId('verdict-word')).toHaveText('Unlikely');
  await expect(page.locator('.verdict [data-note="zooms_better_protein"]')).toContainText('Some chance with ZooMS');
  await expect(page.locator('.verdict [data-note="zooms_better_protein"]')).toContainText('Protein bands show at this spot');
  await expect(page.locator('[data-scan="Spectrum00034.asd"] [data-zooms="zooms_better_protein"]')).toBeVisible();
  await expect(page.getByTestId('zooms-check')).toContainText('ZooMS band check: Borderline');
  // the 1545 nm band
  await page.locator('[data-scan="Spectrum00021.asd"]').click();
  await expect(page.getByTestId('verdict-word')).toHaveText('Unlikely');
  await expect(page.locator('.verdict [data-note="zooms_better_1545"]')).toContainText('The 1545 nm collagen band');
  // the faint sign: quiet, under the verdict only because a slot is free, and no list mark
  await page.locator('[data-scan="Spectrum00026.asd"]').click();
  await expect(page.locator('.verdict [data-note="zooms_faint_protein"]')).toContainText('Faint sign for ZooMS');
  await expect(page.locator('[data-scan="Spectrum00026.asd"] .zmark')).toHaveCount(0);
  // the Ryder 2045 line: Unlikely, bands too noisy, the published model above its ZooMS line
  await page.locator('[data-scan="Spectrum00024.asd"]').click();
  await expect(page.locator('.verdict [data-note="zooms_better_ryder"]')).toContainText('Ryder 2045 model reads 0.35%');
  await expect(page.locator('[data-scan="Spectrum00024.asd"] [data-zooms="zooms_better_ryder"]')).toBeVisible();
  // never on Good
  await page.locator('[data-scan="Spectrum00040.asd"]').click();
  await expect(page.getByTestId('verdict-word')).toHaveText('Good candidate');
  await expect(page.locator('.verdict [data-note^="zooms_better"]')).toHaveCount(0);
  await expect(page.locator('[data-scan="Spectrum00040.asd"] .zmark')).toHaveCount(0);
});

test('a flag shows beside the verdict and never changes it', async ({ page }) => {
  await page.goto('/');
  await page.locator('[data-scan="Spectrum00032.asd"]').click();
  await expect(page.getByTestId('flag')).toContainText('Consolidant sign');
  await expect(page.getByTestId('verdict-word')).toHaveText('Good candidate');
});

test('flipping to Standard recomputes and hides the high-res second opinion', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('[data-model="s1r2"]')).toBeVisible();
  await page.getByRole('button', { name: 'Standard', exact: true }).click();
  await expect(page.locator('[data-model="s1r2"]')).toHaveCount(0);
  await page.getByRole('button', { name: 'High-res', exact: true }).click();
});

test('no "wet" anywhere on screen', async ({ page }) => {
  await page.goto('/');
  const text = (await page.locator('body').innerText()).toLowerCase();
  expect(text).not.toMatch(/\bwet\b/);
});

test('high-res stream toggle, the OH-corrected close-up toggle and Export CSV', async ({ page }) => {
  await page.goto('/');
  const streamGroup = page.getByRole('group', { name: 'Corrected spectrum' });
  await expect(streamGroup.getByRole('button', { name: 'Transferred' })).toHaveAttribute('aria-pressed', 'true');
  await streamGroup.getByRole('button', { name: 'As measured' }).click();
  await expect(page.locator('.ylabel')).toContainText('as measured');
  const lens = page.getByRole('group', { name: '2045 nm view' });
  await expect(lens.getByRole('button', { name: 'OH-corrected' })).toHaveAttribute('aria-pressed', 'true');
  await lens.getByRole('button', { name: 'Uncorrected' }).click();
  await expect(page.locator('.lens p').first()).toContainText('Uncorrected, the altered OH/water band');
  await page.getByTestId('export-csv').click();
  await expect(page.getByRole('status')).toContainText('needs the desktop app');
  await page.getByRole('button', { name: 'Standard', exact: true }).click();
  await expect(page.getByRole('group', { name: 'Corrected spectrum' })).toHaveCount(0);
  await page.getByRole('button', { name: 'High-res', exact: true }).click();
});

test('Help (button and F1) shows the About details and the user guide, never "wet", and closes with Esc', async ({ page }) => {
  await page.goto('/');
  const dlg = page.getByTestId('help-dialog');
  await page.getByTestId('help-btn').click();
  await expect(dlg).toBeVisible();
  await expect(dlg.getByRole('heading', { name: /SPYDER Bone/ }).first()).toBeVisible();
  await expect(dlg.getByText('Results are per scan, never per bone.')).toBeVisible();
  await expect(dlg.locator('table').first()).toBeVisible();
  expect((await dlg.innerText()).toLowerCase()).not.toMatch(/\bwet\b/);
  await page.keyboard.press('Escape');
  await expect(dlg).toBeHidden();
  await page.keyboard.press('F1');
  await expect(dlg).toBeVisible();
  await dlg.getByRole('button', { name: 'Close help' }).click();
  await expect(dlg).toBeHidden();
});

test('the Help dialog does not repeat the guide title under its own header', async ({ page }) => {
  await page.goto('/');
  await page.getByTestId('help-btn').click();
  await expect(page.getByTestId('help-dialog').getByText('SPYDER Bone — User Guide')).toHaveCount(0);
});

test('at the smallest window (1100 × 600) the status bar is on screen', async ({ page }) => {
  await page.setViewportSize({ width: 1100, height: 600 });
  await page.goto('/');
  const bar = await page.locator('footer.statusbar').boundingBox();
  expect(bar).not.toBeNull();
  expect(bar!.y + bar!.height).toBeLessThanOrEqual(600);
});

test('while Help is open, arrow keys do not change the selected scan', async ({ page }) => {
  await page.goto('/');
  const before = await page.locator('.srow[aria-current="true"]').innerText();
  await page.getByTestId('help-btn').click();
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('Escape');
  const after = await page.locator('.srow[aria-current="true"]').innerText();
  expect(after).toBe(before);
});

test('the band close-ups show every band and the ZooMS band check, no score number; the details panel no longer repeats them', async ({ page }) => {
  await page.goto('/');
  await page.locator('[data-scan="Spectrum00021.asd"]').click();
  const bands = page.locator('.bandrow [data-band]');
  await expect(bands).toHaveCount(7);
  // 1545* is read on the OH-corrected curve: said in its label's accessible name and in its tooltip (hover and focus)
  const b1545 = page.locator('.bandrow [data-band="nh1545"]');
  await expect(b1545).toContainText('1545*');
  await expect(b1545).toHaveAttribute('aria-label', /collagen shows here as a dip, not a peak/);
  await b1545.focus();
  await expect(b1545.locator('.tip')).toBeVisible();
  await expect(page.locator('.bandsum')).not.toContainText('score');
  await expect(page.locator('.bandsum').getByTestId('zooms-check')).toContainText('ZooMS band check: Borderline');
  await expect(page.locator('.details')).not.toContainText('Evidence of organics');
  // the technical notes are one click away
  await expect(page.locator('.bandfoot .about ul')).toBeHidden();
  await page.locator('.bandfoot .about summary').click();
  await expect(page.locator('.bandfoot .about ul')).toContainText('can only add evidence');
  // a long file name keeps its end in the list and shows in full on hover
  const row = page.locator('[data-scan="SiteA_Trench2_Layer4b_long-bone-fragment_0117_spot3_avg200.asd"]');
  await expect(row.locator('.name')).toHaveAttribute('title', 'SiteA_Trench2_Layer4b_long-bone-fragment_0117_spot3_avg200.asd');
  await expect(row.locator('.nt')).toHaveText('_avg200');
});

test('a charred heat sign is marked in Reflectance; the 2nd-derivative toolbar has no button for it (Matt, 2026-10-18 night)', async ({ page }) => {
  await page.goto('/#scan=Spectrum00023');
  const card = page.locator('.chartcard');
  await card.getByRole('button', { name: '2nd derivative', exact: true }).click();
  await expect(card.locator('.toolbar')).not.toContainText('Heat sign');
  await card.getByRole('button', { name: 'Reflectance', exact: true }).click();
  await expect(card.locator('.legend')).toContainText('Heat sign');
});

// The close-up band labels (Matt, 2026-10-18 night: labels moved off their bands as the window shrank; 2026-10-19: numbers
// staggered on two lines read as "a ragged staircase of numbers, not as labels"). One line of numbers in every close-up:
// each number centred on its band, or a pair too close for that set as one label centred on the pair; each glyph on its
// band; numbers inside their box; titles, numbers and glyphs on shared lines across the row. Checked the way Matt runs the
// app: Windows display scaling 1.25 and 1.5, 1100 to 1920 px wide in 20 px steps, the default and a widened scans list
// (the row then goes under the verdict on the smaller windows).
// Font-independent: the checks assert the rules, not which pairs merge at which width (that follows the measured text
// widths), and the sweep runs again in a wide font set after the page has drawn (DejaVu Sans, Linux's fallback, or
// Verdana with the same metrics: wider than Segoe UI and SF Pro), so the labels must re-measure and still fit.
const WIDE_FONT = ':root { --font-ui: "DejaVu Sans", Verdana, sans-serif !important; }';
type LabelMetrics = {
  heroRight: number;
  cus: {
    family: string;
    l: number;
    r: number;
    titleTop: number;
    titleOff: number;
    sep: { l: number; r: number } | null;
    nums: { id: string; band: number; c: number; t: number; l: number; r: number; g: number; gt: number; fs: number }[];
  }[];
};
const measureLabels = (page: import('@playwright/test').Page) =>
  page.evaluate((): LabelMetrics => {
    const rect = (e: Node) => {
      const r = document.createRange();
      r.selectNodeContents(e);
      return r.getBoundingClientRect();
    };
    const hero = document.querySelector('.hero')!.getBoundingClientRect();
    const cus = [...document.querySelectorAll('.bandrow .cu')].map((cu) => {
      const b = cu.getBoundingClientRect();
      const bw = parseFloat(getComputedStyle(cu).borderLeftWidth);
      const plots = [...cu.querySelectorAll('.cuplot')].map((p) => p.getBoundingClientRect());
      const pc = (Math.min(...plots.map((p) => p.left)) + Math.max(...plots.map((p) => p.right))) / 2;
      const title = [...cu.querySelectorAll('.cut, .cut > span')].filter((e) => e.children.length === 0 && getComputedStyle(e).display !== 'none')[0];
      const tr = rect(title);
      const nums: LabelMetrics['cus'][number]['nums'] = [];
      let sep: { l: number; r: number } | null = null;
      for (const win of cu.querySelectorAll('.win')) {
        const [lo, hi] = win.getAttribute('data-range')!.split('-').map(Number);
        const p = win.querySelector('.cuplot')!.getBoundingClientRect();
        const s = win.querySelector('.sep');
        if (s) {
          const sr = rect(s);
          sep = { l: sr.left, r: sr.right };
        }
        for (const tag of win.querySelectorAll('.tag')) {
          const nm = Number(tag.getAttribute('data-nm'));
          const d = rect(tag.querySelector('.nm')!.firstChild!);
          const ast = tag.querySelector('.ast');
          const g = tag.querySelector('.g')!.getBoundingClientRect();
          nums.push({
            id: tag.getAttribute('data-band')!,
            band: p.left + 4 + ((nm - lo) / (hi - lo)) * (p.width - 8),
            c: (d.left + d.right) / 2,
            t: d.top,
            l: d.left,
            r: ast ? ast.getBoundingClientRect().right : d.right,
            g: (g.left + g.right) / 2,
            gt: g.top,
            fs: parseFloat(getComputedStyle(tag).fontSize),
          });
        }
      }
      return { family: title.textContent!, l: b.left + bw, r: b.right - bw, titleTop: tr.top, titleOff: (tr.left + tr.right) / 2 - pc, sep, nums };
    });
    return { heroRight: hero.right, cus };
  });

/** Every rule the labels must keep; returns the broken ones, in words. */
function labelFaults(m: LabelMetrics): string[] {
  const f: string[] = [];
  const spread = (xs: number[]) => Math.max(...xs) - Math.min(...xs);
  const all = m.cus.flatMap((c) => c.nums);
  if (all.length !== 7) f.push(`${all.length} band labels, not 7`);
  if (m.cus.length && m.cus[m.cus.length - 1].r > m.heroRight + 0.5) f.push('the close-ups overflow the card');
  if (spread(m.cus.map((c) => c.titleTop)) > 1) f.push('titles on different lines');
  if (spread(all.map((n) => n.t)) > 1) f.push('numbers on different lines across the row');
  if (spread(all.map((n) => n.gt)) > 1) f.push('glyphs on different lines across the row');
  for (const c of m.cus) {
    if (Math.abs(c.titleOff) > 1) f.push(`${c.family}: title ${c.titleOff.toFixed(1)} px off its plots`);
    // never a staircase: every number of a close-up on one line
    if (spread(c.nums.map((n) => n.t)) > 1) f.push(`${c.family}: numbers on two lines`);
    for (const n of c.nums) {
      if (Math.abs(n.g - n.band) > 1) f.push(`${n.id}: glyph ${(n.g - n.band).toFixed(1)} px off its band`);
      if (n.fs < 10) f.push(`${n.id}: font ${n.fs} px`);
      if (n.l < c.l - 0.5 || n.r > c.r + 0.5) f.push(`${n.id}: number outside its close-up`);
    }
    const ns = [...c.nums].sort((a, b) => a.band - b.band);
    for (let i = 1; i < ns.length; i++) if (ns[i].l < ns[i - 1].r + 2) f.push(`${ns[i - 1].id} and ${ns[i].id} touch`);
    if (c.sep) {
      // one label for the pair, centred on the pair, the separator between the two numbers
      const off = (ns[0].l + ns[1].r) / 2 - (ns[0].band + ns[1].band) / 2;
      if (Math.abs(off) > 1) f.push(`${c.family}: pair label ${off.toFixed(1)} px off the pair`);
      if (!(ns[0].r <= c.sep.l && c.sep.r <= ns[1].l)) f.push(`${c.family}: separator not between the numbers`);
    } else for (const n of c.nums) if (Math.abs(n.c - n.band) > 1) f.push(`${n.id}: number ${(n.c - n.band).toFixed(1)} px off its band`);
  }
  return f;
}

for (const [scale, font] of [[1.25, null], [1.5, null], [1.5, WIDE_FONT]] as const)
  for (const list of [null, '560'] as const)
    test(`the close-up labels: one tidy line on their bands, 1100–1920 px at scale ${scale}${list ? ', widened scans list' : ''}${font ? ', a wide font' : ''}`, async ({ browser }) => {
      test.setTimeout(180_000);
      const ctx = await browser.newContext({ viewport: { width: 1100, height: 700 }, deviceScaleFactor: scale });
      const page = await ctx.newPage();
      await page.goto('/');
      await page.evaluate((l) => (l ? localStorage.setItem('spyder.listWidth', l) : localStorage.removeItem('spyder.listWidth')), list);
      await page.goto('/#scan=Spectrum00021');
      await page.reload();
      await page.waitForSelector('.bandrow .cuplot .uplot');
      if (font) await page.addStyleTag({ content: font });
      const faults: string[] = [];
      let under = 0;
      for (let w = 1100; w <= 1920; w += 20) {
        await page.setViewportSize({ width: w, height: Math.round(w * 0.5625) });
        // settle: measure until two frames agree (the numbers are placed from measured widths)
        let m = await measureLabels(page);
        for (let i = 0; i < 20; i++) {
          await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
          const n = await measureLabels(page);
          const same = JSON.stringify(n) === JSON.stringify(m);
          m = n;
          if (same) break;
        }
        const isUnder = await page.evaluate(
          () => document.querySelector('.bandrow')!.getBoundingClientRect().top >= document.querySelector('.hero .verdict')!.getBoundingClientRect().bottom - 2,
        );
        if (isUnder) under++;
        for (const x of labelFaults(m)) faults.push(`${w} px: ${x}`);
      }
      expect(faults).toEqual([]);
      // the widened list puts the row under the verdict on the smaller windows: both layouts are covered
      if (list) expect(under).toBeGreaterThan(0);
      await ctx.close();
    });

test('a close pair reads as one label where its numbers cannot sit apart; each number lights its own band', async ({ page }) => {
  await page.setViewportSize({ width: 1640, height: 880 });
  await page.goto('/#scan=Spectrum00021');
  await page.waitForSelector('.bandrow .cuplot .uplot');
  // each two-band window: the bands' distance against the numbers' widths, as drawn
  const pairs = await page.evaluate(() =>
    [...document.querySelectorAll('.bandrow .win')]
      .filter((w) => w.querySelectorAll('.tag').length === 2)
      .map((w) => {
        const [a, b] = [...w.querySelectorAll('.tag')];
        const nw = (t: Element) => t.querySelector('.nm')!.getBoundingClientRect().width;
        return {
          id: a.getAttribute('data-band')!,
          dist: b.getBoundingClientRect().left - a.getBoundingClientRect().left,
          half: (nw(a) + nw(b)) / 2,
          sep: !!w.querySelector('.sep'),
        };
      }),
  );
  expect(pairs.map((p) => p.id)).toEqual(['ch1689', 'ch2262']);
  for (const p of pairs) {
    // numbers centred on bands closer than their half-widths would overlap: there the pair label must be used. Where
    // both forms are legitimate (1689 / 1728 in a wide close-up), either is accepted; the sweep above checks the rules.
    if (p.dist < p.half + 2) expect(p.sep, `${p.id}: the pair label`).toBe(true);
  }
  // 2262 and 2284 are 22 nm apart in a 67 nm window: in any font the pair label
  expect(pairs[1].dist).toBeLessThan(pairs[1].half + 2);
  expect(labelFaults(await measureLabels(page))).toEqual([]);
  await expect(page.locator('.bandrow .win:has([data-band="nh2044"]) .sep')).toHaveCount(0);
  // hovering either number of the pair lights its own band (and its marks on the main spectrum)
  await page.locator('[data-band="ch2284"] .nm').hover();
  await expect(page.locator('[data-band="ch2284"]')).toHaveClass(/hl/);
  await expect(page.locator('[data-band="ch2262"]')).not.toHaveClass(/hl/);
});

test('row marks are symbols with their full text, and the scans list can be widened by keyboard and reset', async ({ page }) => {
  await page.goto('/');
  await page.evaluate(() => localStorage.removeItem('spyder.listWidth'));
  await page.reload();
  const row = page.locator('[data-scan="Session2_femur-shaft_lateral_after-brushing_rep1.asd"]');
  // the flag, ZooMS and panel marks: no visible words, the full text in the tooltip and the accessible name
  await expect(row.locator('.mk.flag')).toHaveAttribute('aria-label', /Heat sign/);
  await expect(row.locator('.mk.zmark')).toHaveAttribute('title', /ZooMS/);
  await expect(row.locator('.mk.pmark')).toHaveAttribute('aria-label', /disagree strongly/);
  await expect(row.locator('.mk')).toHaveText(['', '', '']);
  // the reading is never cut
  await expect(page.locator('[data-scan="Spectrum00037.asd"] .val')).toHaveText('No protein signal');
  const list = page.locator('aside.scans');
  const w0 = (await list.boundingBox())!.width;
  const grip = page.getByRole('slider', { name: 'Scans list width' });
  await grip.focus();
  await page.keyboard.press('ArrowRight');
  await page.keyboard.press('ArrowRight');
  await expect.poll(async () => (await list.boundingBox())!.width).toBe(w0 + 32);
  expect(await page.evaluate(() => localStorage.getItem('spyder.listWidth'))).toBe(String(w0 + 32));
  await page.keyboard.press('Enter');
  await expect.poll(async () => (await list.boundingBox())!.width).toBe(w0);
  expect(await page.evaluate(() => localStorage.getItem('spyder.listWidth'))).toBeNull();
});

test('the details column: how the verdict was made under the consensus, the standing caveats one click away, nothing repeated', async ({ page }) => {
  await page.goto('/');
  await page.locator('[data-scan="Spectrum00039.asd"]').click();
  const d = page.locator('aside.details');
  await expect(d).toContainText('Starting estimate; band evidence can change the verdict.');
  await expect(d).toContainText('Additional estimates · not used for the verdict');
  await expect(d).not.toContainText('Display smoothing');
  await expect(d).not.toContainText('transferred to standard resolution (blur');
  // closed by default: the scan's own numbers, the general rules, the standing caveats
  await expect(d.getByText('Glues and thin coats')).toBeHidden();
  await d.getByText('Limits of the checks').click();
  await expect(d.getByText('Glues and thin coats')).toBeVisible();
  await d.getByText('How this verdict was made').click();
  await expect(d.getByTestId('verdict-number').first()).toContainText('Rule applied');
  await expect(d.locator('.rules .ht', { hasText: 'Screening lines' })).toBeHidden();
  // a target that is not bone: labelled, never dimmed
  await page.locator('[data-scan="Spectrum00029.asd"]').click();
  await expect(page.getByTestId('not-interpretable')).toContainText('Not interpretable for this target');
});

test('the strong panel notes are warnings: a box on the verdict card, a mark that gives way only after ZooMS', async ({ page }) => {
  await page.goto('/');
  await page.locator('[data-scan="Session2_femur-shaft_lateral_after-brushing_rep1.asd"]').click();
  await expect(page.getByTestId('warning')).toContainText('Collagen estimates disagree strongly');
  await expect(page.getByTestId('warning')).toContainText('Treat the percentage with caution');
  // in the row, the warning mark sits after the flag and before ZooMS (the order the marks give way, last first)
  const marks = page.locator('[data-scan="Session2_femur-shaft_lateral_after-brushing_rep1.asd"] .mk');
  await expect(marks).toHaveCount(3);
  await expect(marks.nth(0)).toHaveClass(/flag/);
  await expect(marks.nth(1)).toHaveClass(/pmark/);
  await expect(marks.nth(2)).toHaveClass(/zmark/);
  // the milder "models disagree" note is details-only (DECISIONS 136): never on the card, listed in the details
  await page.locator('[data-scan="Spectrum00026.asd"]').click();
  await expect(page.locator('.verdict [data-note="models_disagree"]')).toHaveCount(0);
  await expect(page.locator('aside.details [data-note="models_disagree"]')).toBeVisible();
});

test('a fired strong warning is always on the verdict card, beside a quieter note, and never repeated in the details', async ({ page }) => {
  await page.goto('/');
  // a quiet supporting note keeps its slot; the readers warning is on the card too, naming the pair and its readings
  await page.locator('[data-scan="Spectrum00041.asd"]').click();
  const card = page.locator('.verdict');
  await expect(card.locator('.note[data-note="possible_thin_coating"]')).toBeVisible();
  await expect(card.getByTestId('warning')).toHaveCount(1);
  await expect(card.getByTestId('warning')).toContainText('Collagen estimates disagree strongly');
  await expect(card.getByTestId('warning')).toContainText('The narrow 1545 nm band reads 6.6% against the consensus 13.6%');
  const details = page.locator('aside.details');
  await expect(details.locator('[data-note="readers_disagree_strongly"]')).toHaveCount(0);
  await expect(details.locator('[data-note="possible_thin_coating"]')).toHaveCount(0);
  await expect(details.locator('[data-note="ch_above_nh_possible"]')).toBeVisible();
  // the most crowded card: a flag, the rule's note, the ZooMS line and both warnings, the C–H warning first
  await page.locator('[data-scan="Spectrum00042.asd"]').click();
  await expect(card.getByTestId('flag')).toHaveCount(1);
  await expect(card.locator('.note[data-note="lift_borderline"]')).toBeVisible();
  await expect(card.locator('.note[data-note="zooms_better_good"]')).toBeVisible();
  const warnings = card.getByTestId('warning');
  await expect(warnings).toHaveCount(2);
  await expect(warnings.nth(0)).toHaveAttribute('data-note', 'ch_above_nh_strong');
  await expect(warnings.nth(1)).toHaveAttribute('data-note', 'readers_disagree_strongly');
  await expect(details.locator('[data-note="ch_above_nh_strong"], [data-note="readers_disagree_strongly"]')).toHaveCount(0);
  // a plain scan whose only note is the warning
  await page.locator('[data-scan="Spectrum00043.asd"]').click();
  await expect(card.getByTestId('warning')).toContainText('The three consensus models read 2045 nm 3.2%');
  await expect(card.locator('.note')).toHaveCount(0);
});

for (const [w, h] of [
  [1100, 700],
  [1280, 800],
] as const) {
  test(`the compact card never pushes the spectrum down (${w}×${h}): the crowded card's spectrum starts where a plain scan's does`, async ({ browser }) => {
    const ctx = await browser.newContext({ viewport: { width: w, height: h } });
    const p = await ctx.newPage();
    const top = async (scan: string) => {
      await p.goto(`/#scan=${scan}`);
      await p.reload();
      await p.waitForSelector('.uplot');
      return p.evaluate(() => {
        const card = document.querySelector('.verdict') as HTMLElement;
        const shown = [...card.children].filter((k) => (k as HTMLElement).offsetParent !== null);
        return {
          spectrum: Math.round(document.querySelector('.hero + *')!.getBoundingClientRect().top),
          spare: Math.round(card.getBoundingClientRect().bottom - shown[shown.length - 1].getBoundingClientRect().bottom),
        };
      });
    };
    const plain = await top('Spectrum00040');
    const crowded = await top('Spectrum00042');
    expect(crowded.spectrum).toBe(plain.spectrum);
    // the crowded card's own content ends inside the card: the close-ups, not the card, set the row's height
    expect(crowded.spare).toBeGreaterThanOrEqual(11);
    // every chip carries its caret, and the longest heading stays on one line with it
    const chips = p.locator('.verdict .item button');
    await expect(chips).toHaveCount(5);
    await expect(p.locator('.verdict .item button .caret')).toHaveCount(5);
    const long = p.locator('.verdict [data-note="readers_disagree_strongly"] button');
    expect((await long.boundingBox())!.height).toBeLessThan(26);
    await ctx.close();
  });
}

test('every compact item: the full text on hover and keyboard focus, Escape hides it, click or Enter opens it in place', async ({ page }) => {
  await page.goto('/');
  await page.locator('[data-scan="Spectrum00042.asd"]').click();
  const w = page.locator('.verdict [data-testid="warning"]').nth(1);
  const b = w.locator('button');
  const tip = w.locator('[role="tooltip"]');
  await expect(b).toHaveText('Collagen estimates disagree strongly');
  await expect(b).toHaveAttribute('aria-expanded', 'false');
  // keyboard focus shows the tooltip (a native title would not)
  await b.focus();
  await page.keyboard.press('Shift+Tab');
  await page.keyboard.press('Tab');
  await expect(tip).toBeVisible();
  await expect(tip).toContainText('Ryder 2045 reads 2.1% against the consensus 0.4%');
  await page.keyboard.press('Escape');
  await expect(tip).toBeHidden();
  // Enter opens it in place (and the tooltip stays out of the way)
  await page.keyboard.press('Enter');
  await expect(b).toHaveAttribute('aria-expanded', 'true');
  const panel = page.locator(`#${await b.getAttribute('aria-controls')}`);
  await expect(panel).toBeVisible();
  await expect(panel).toContainText('Treat the percentage with caution');
  await b.click();
  await expect(panel).toBeHidden();
  // hover shows a flag's full text too
  const flag = page.locator('.verdict [data-testid="flag"]');
  await flag.locator('button').hover();
  await expect(flag.locator('[role="tooltip"]')).toBeVisible();
  await expect(flag.locator('[role="tooltip"]')).toContainText('Heating lowers collagen');
  // the low-range explanation and, on a card with a warning, the verdict's sentence are one interaction away too
  const lr = page.locator('.verdict .lowrange button');
  await lr.click();
  await expect(page.getByTestId('lowrange-note')).toBeVisible();
  await page.locator('[data-scan="Spectrum00041.asd"]').click();
  await expect(page.locator('.verdict .vnote')).toBeHidden();
  await page.getByTestId('verdict-word').click();
  await expect(page.locator('.verdict .vnote')).toContainText('Collagen looks ample');
  // with no flag or warning the sentence is in view
  await page.locator('[data-scan="Spectrum00040.asd"]').click();
  await expect(page.locator('.verdict .vnote')).toBeVisible();
});

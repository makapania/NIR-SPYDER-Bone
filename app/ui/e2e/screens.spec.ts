import { test } from '@playwright/test';

// Screenshots for review. Runs only when SCREENS_DIR is set:
//   SCREENS_DIR=../../planning/work/design/screens_app_v2 PW_CHANNEL=msedge npx playwright test --project=screens
// Optional SCREENS_ONLY=<substring> limits the run to matching names.
const dir = process.env.SCREENS_DIR;
const only = process.env.SCREENS_ONLY;

interface Size {
  w: number;
  h: number;
  dpr: number;
}
/** After load: 'focus' tabs to the card's second warning (its tooltip), 'open' clicks it open in place. */
type Act = 'focus' | 'open';
const shots: [string, string, Size?, Act?][] = [
  ['01-dark-good', ''],
  ['02-dark-borderline-zooms-line-positive-signs', 'scan=Spectrum00039'],
  ['03-dark-consolidant-flag', 'scan=Spectrum00032'],
  ['04-dark-heat-flag', 'scan=Spectrum00023'],
  ['05-dark-lifted', 'scan=Spectrum00031'],
  ['06-dark-blocked-lift-wax-flag', 'scan=Spectrum00030'],
  ['07-dark-flat-bands', 'scan=Spectrum00037'],
  ['08-dark-noisy-contaminants-not-checked', 'scan=Spectrum00036'],
  // the ZooMS line (DECISIONS 80 amended): protein bands, the 1545 nm band (all six bands: 02), and the list marks
  ['09-dark-zooms-line-protein', 'scan=Spectrum00034'],
  ['10-dark-zooms-line-1545', 'scan=Spectrum00021'],
  ['18-dark-zooms-faint-protein', 'scan=Spectrum00026'],
  ['19-dark-flat-bands-1545', 'scan=Spectrum00028'],
  ['20-dark-zooms-ryder-line', 'scan=Spectrum00024'],
  ['11-dark-promising', 'promising'],
  ['12-dark-not-bone', 'scan=Spectrum00029'],
  ['13-dark-rescan', 'scan=Spectrum00027'],
  ['14-dark-as-measured-altered-oh', 'asmeasured&scan=Spectrum00026'],
  ['15-dark-second-opinion', 'scan=Spectrum00025'],
  ['16-dark-colour-blind-safe', 'cvd&scan=Spectrum00039'],
  ['17-dark-standard-switch', 'scan=Spectrum00040&standard'],
  ['21-light-good', 'light'],
  ['22-light-borderline-zooms-line-positive-signs', 'light&scan=Spectrum00039'],
  ['23-light-consolidant-flag', 'light&scan=Spectrum00032'],
  ['24-light-zooms-line-promising', 'light&promising&scan=Spectrum00034'],
  ['25-light-promising', 'light&promising'],
  // layout checks: 150% Windows scaling on a 1920×1080 screen (1280×720 CSS px), and 1366×768
  ['30-dark-150pct-1280x720-good', '', { w: 1280, h: 720, dpr: 1.5 }],
  ['31-dark-150pct-1280x720-wax-flag', 'scan=Spectrum00030', { w: 1280, h: 720, dpr: 1.5 }],
  ['32-dark-1366x768-consolidant-flag', 'scan=Spectrum00032', { w: 1366, h: 768, dpr: 1 }],
  ['33-light-150pct-1280x720-cvd', 'light&cvd&scan=Spectrum00039', { w: 1280, h: 720, dpr: 1.5 }],
  // the band close-ups as the one place to see every band (2026-10-18): a row of tall plots under the verdict on laptop
  // widths, beside it on wide windows; both themes, every band state
  ['40-closeups-dark-1280x800-all-six', 'scan=Spectrum00039', { w: 1280, h: 800, dpr: 1 }],
  ['41-closeups-dark-1100x700-1545-dip', 'scan=Spectrum00021', { w: 1100, h: 700, dpr: 1 }],
  ['42-closeups-light-1280x800-all-six', 'light&scan=Spectrum00039', { w: 1280, h: 800, dpr: 1 }],
  ['43-closeups-light-1100x700-1545-dip', 'light&scan=Spectrum00021', { w: 1100, h: 700, dpr: 1 }],
  ['44-closeups-dark-1280x800-consolidant-flag', 'scan=Spectrum00032', { w: 1280, h: 800, dpr: 1 }],
  ['45-closeups-dark-1280x800-too-noisy', 'scan=Spectrum00036', { w: 1280, h: 800, dpr: 1 }],
  ['46-closeups-dark-1280x800-flat-bands', 'scan=Spectrum00037', { w: 1280, h: 800, dpr: 1 }],
  ['47-closeups-dark-1280x800-bands-down', 'bandsdown&scan=Spectrum00021', { w: 1280, h: 800, dpr: 1 }],
  ['48-closeups-dark-1100x700-cvd', 'cvd&scan=Spectrum00034', { w: 1100, h: 700, dpr: 1 }],
  ['49-closeups-dark-150pct-1280x720-1545-dip', 'scan=Spectrum00021', { w: 1280, h: 720, dpr: 1.5 }],
  ['50-closeups-light-150pct-1280x720-wax-flag', 'light&scan=Spectrum00030', { w: 1280, h: 720, dpr: 1.5 }],
  ['51-closeups-dark-1920x1080-beside-the-verdict', 'scan=Spectrum00039', { w: 1920, h: 1080, dpr: 1 }],
  ['53-closeups-dark-1536x864-beside-the-verdict', 'scan=Spectrum00039', { w: 1536, h: 864, dpr: 1 }],
  ['52-closeups-light-1100x700-bands-down-cvd', 'light&cvd&bandsdown&scan=Spectrum00034', { w: 1100, h: 700, dpr: 1 }],
  // the scans list's band summary (the bars, DECISIONS 126)
  ['55-list-bars-dark-1440x900', 'scan=Spectrum00039'],
];

// The strong warnings' place on the verdict card (design/BRIEF_warning_place.md): a quiet supporting note with the readers
// warning (41), the most crowded card (42: a flag, the rule's note, the ZooMS line, a supporting note and both warnings) and a
// plain scan with only the warning (43); laptop widths, both themes and the colour-blind palette.
// SCREENS_WARN=<label>[=<hash>],... adds them with an optional extra hash, e.g. SCREENS_WARN=final (the three placement
// options compared were #warn=a, b and c at f4291e7, the compact card's #card=a, b and c at ef40ecd), plus the crowded
// card with a warning's tooltip (keyboard focus) and with it opened in place.
for (const spec of (process.env.SCREENS_WARN ?? '').split(',').filter(Boolean)) {
  const [label, ...rest] = spec.split('=');
  const extra = rest.join('=');
  for (const [cn, cs] of [['41', 'quiet-note'], ['42', 'crowded'], ['43', 'plain']] as const)
    for (const [tn, th] of [['dark', ''], ['light', 'light'], ['cvd', 'cvd']] as const)
      for (const [w, h] of [[1100, 700], [1280, 800]] as const)
        shots.push([
          `wp-${label}-${cs}-${tn}-${w}`,
          [`scan=Spectrum000${cn}`, th, extra].filter(Boolean).join('&'),
          { w, h, dpr: 1 },
        ]);
  for (const act of ['focus', 'open'] as const)
    for (const [tn, th] of [['dark', ''], ['light', 'light']] as const)
      shots.push([`wp-${label}-crowded-${tn}-1280-${act}`, ['scan=Spectrum00042', th, extra].filter(Boolean).join('&'), { w: 1280, h: 800, dpr: 1 }, act]);
}

test.describe('screens', () => {
  test.skip(!dir, 'SCREENS_DIR not set');
  for (const [name, hash, size, act] of shots) {
    if (only && !name.includes(only)) continue;
    test(name, async ({ page, browser }) => {
      let p = page;
      if (size) {
        const ctx = await browser.newContext({ viewport: { width: size.w, height: size.h }, deviceScaleFactor: size.dpr });
        p = await ctx.newPage();
      }
      await p.goto(`http://localhost:${process.env.PW_PORT ?? '5173'}/#${hash}`);
      await p.waitForSelector('.uplot');
      await p.waitForTimeout(700);
      if (act) {
        const w = p.locator('.verdict [data-testid="warning"]').nth(1);
        const b = w.locator('button').first();
        if (act === 'focus') {
          // keyboard focus, so the tooltip shows as it does for a keyboard user
          await b.focus();
          await p.keyboard.press('Shift+Tab');
          await p.keyboard.press('Tab');
        } else await b.click();
        await p.mouse.move(5, 795);
        await p.waitForTimeout(250);
      }
      await p.screenshot({ path: `${dir}/${name}.png` });
      if (size) await p.context().close();
    });
  }
});

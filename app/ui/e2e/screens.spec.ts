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
const shots: [string, string, Size?][] = [
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
];

test.describe('screens', () => {
  test.skip(!dir, 'SCREENS_DIR not set');
  for (const [name, hash, size] of shots) {
    if (only && !name.includes(only)) continue;
    test(name, async ({ page, browser }) => {
      let p = page;
      if (size) {
        const ctx = await browser.newContext({ viewport: { width: size.w, height: size.h }, deviceScaleFactor: size.dpr });
        p = await ctx.newPage();
      }
      await p.goto(`http://localhost:5173/#${hash}`);
      await p.waitForSelector('.uplot');
      await p.waitForTimeout(700);
      await p.screenshot({ path: `${dir}/${name}.png` });
      if (size) await p.context().close();
    });
  }
});

import { expect, test } from '@playwright/test';

test('core screen renders a verdict, the scan list and the charts from the mock', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByTestId('verdict-word')).toHaveText('Good candidate');
  await expect(page.locator('.srow')).toHaveCount(20);
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
  await expect(page.locator('.verdict [data-note="zooms_better_protein"]')).toContainText('Some chance with ZooMS: protein bands');
  await expect(page.locator('[data-scan="Spectrum00034.asd"] [data-zooms="zooms_better_protein"]')).toBeVisible();
  await expect(page.getByTestId('zooms-check')).toContainText('ZooMS band check: Borderline');
  // the 1545 nm band
  await page.locator('[data-scan="Spectrum00021.asd"]').click();
  await expect(page.getByTestId('verdict-word')).toHaveText('Unlikely');
  await expect(page.locator('.verdict [data-note="zooms_better_1545"]')).toContainText('the 1545 nm collagen band');
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
  const streamGroup = page.getByRole('group', { name: 'High-res spectrum' });
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
  await expect(page.getByRole('group', { name: 'High-res spectrum' })).toHaveCount(0);
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

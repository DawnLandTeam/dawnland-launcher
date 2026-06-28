import { test, expect } from './fixtures';

test.describe('Updater (E2E)', () => {
  test('Launcher update prompt', async ({ page }) => {
    // Navigate to settings
    await page.waitForLoadState('domcontentloaded');

    const settingsNav = page.locator('a[href="/settings"]').first();
    if (await settingsNav.isVisible()) await settingsNav.click();

    // Check for "Check for Updates" button
    const checkUpdateBtn = page.locator('button:has-text("检查更新"), button:has-text("Check for Updates")').first();
    if (await checkUpdateBtn.isVisible()) {
      await checkUpdateBtn.click();
      
      // If it's already the latest, it will say "Up to date" or similar
      // Since this is testing against actual release, it depends on the actual github API
      // We just verify it doesn't crash and shows some toast/result
      const resultToast = page.locator(':has-text("已是最新版本"), :has-text("最新"), :has-text("Up to date")').last();
      await expect(resultToast).toBeVisible({ timeout: 10000 });
    }
  });
});

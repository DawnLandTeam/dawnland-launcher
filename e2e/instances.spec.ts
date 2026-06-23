import { test, expect } from './fixtures';

test.describe('Instances View', () => {
  test('should render installed instances and context menus', async ({ page, mockTauri }) => {
    await mockTauri.setMockResponses({
      'scan_installed_instances': [
        { id: 'inst-1', name: 'Vanilla 1.20', mcVersion: '1.20', loaderType: 'vanilla' },
        { id: 'inst-2', name: 'Forge Pack', mcVersion: '1.19.2', loaderType: 'forge', modpackType: 'curseforge' }
      ]
    });

    await page.goto('/instances');

    // Wait for the elements to be visible
    await expect(page.getByText('Vanilla 1.20')).toBeVisible();
    await expect(page.getByText('Forge Pack')).toBeVisible();

    // Check loader badges
    await expect(page.getByText('vanilla', { exact: true })).toBeVisible();
    await expect(page.getByText('forge', { exact: true })).toBeVisible();

    // Context Menu / Options
    // Find the first "More options" button
    const moreOptionsBtns = page.locator('button[title="More options"]');
    await expect(moreOptionsBtns.first()).toBeVisible();
    
    await moreOptionsBtns.first().click();

    // Should see Settings, Open Folder, Delete
    // Note: Depends on locale, we use regex or generic locators
    // Just finding the Settings/Folder icons usually works, or the specific text
    await expect(page.locator('.lucide-settings').last()).toBeVisible();
    await expect(page.locator('.lucide-folder').last()).toBeVisible();
  });

  test('should open install modal', async ({ page, mockTauri }) => {
    await mockTauri.setMockResponses({
      'scan_installed_instances': []
    });

    await page.goto('/instances');

    // Click the Add button
    // Either "Install Instance" (empty state) or Add in the header
    const addBtn = page.locator('button', { hasText: /install|add|新建/i }).first();
    await expect(addBtn).toBeVisible();
    await addBtn.click();

    // Verify we navigated to the downloads page with instance tab
    await expect(page).toHaveURL(/.*downloads\?tab=instance/);
  });
});

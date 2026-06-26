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

    // Open settings by clicking the instance card
    const instanceCard = page.locator('.group.flex.flex-col').first();
    await expect(instanceCard).toBeVisible();
    await instanceCard.click();

    // Should see Settings in the management view
    await expect(page).toHaveURL(/.*instances\/inst.*/);
    await expect(page.locator('.lucide-settings').last()).toBeVisible();
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

import { test, expect } from './fixtures';

test.describe('Navigation & Core Layout', () => {
  test('should render sidebar and navigate between views', async ({ page, mockTauri }) => {
    // Setup initial mock responses so views don't hang
    await mockTauri.setMockResponses({
      'get_accounts': [],
      'scan_installed_instances': [],
      'get_servers': [],
      'scan_local_javas': [],
    });

    await page.goto('/');

    // Ensure sidebar is present
    const sidebar = page.locator('aside');
    await expect(sidebar).toBeVisible();

    // Check Home View
    await expect(page.locator('a[href="/"]')).toBeVisible();

    // Navigate to Instances
    await page.click('a[href="/instances"]');
    await expect(page).toHaveURL(/\/instances/);
    
    // Navigate to Servers
    await page.click('a[href="/servers"]');
    await expect(page).toHaveURL(/\/servers/);

    // Navigate to Accounts
    await page.click('a[href="/accounts"]');
    await expect(page).toHaveURL(/\/accounts/);

    // Navigate to Settings
    await page.click('a[href="/settings"]');
    await expect(page).toHaveURL(/\/settings/);
    
    // Check Task Center Toggle
    const taskCenterBtn = page.locator('button.task-center-toggle');
    await expect(taskCenterBtn).toBeVisible();
  });
});

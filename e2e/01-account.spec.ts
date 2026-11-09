import { test, expect } from './fixtures';

test.describe('Account Management (E2E)', () => {
  test('Add, view details, navigate, and remove an offline account', async ({ page }) => {
    // Click the Accounts nav item in sidebar
    const accountsNav = page.locator('a[title="账号"], a[title="Accounts"], a[href="/accounts"]').first();
    await accountsNav.click({ timeout: 15000 });

    // Now on Accounts View: click "Add Account"
    const addAccountBtn = page.locator('button:has-text("添加账号"), button:has-text("Add Account")').first();
    await expect(addAccountBtn).toBeVisible({ timeout: 5000 });
    await addAccountBtn.click();
    
    // Click the "Offline" tab in the dialog
    const offlineTab = page.locator('button:has-text("离线账号"), button:has-text("Offline")').first();
    await offlineTab.click();

    // Type the username
    const usernameInput = page.locator('input[placeholder*="输入用户名"], input[placeholder*="Enter username"]');
    await usernameInput.fill('E2E_Tester');

    // Click Add button inside dialog
    const submitBtn = page.locator('button:has-text("添加"), button:has-text("Add")').last();
    await submitBtn.click();

    // Assert that the account appears in the list
    const accountCard = page.locator('div.border', { hasText: 'E2E_Tester' }).first();
    await expect(accountCard).toBeVisible({ timeout: 5000 });

    // Click on the account card body to open Account Detail Modal
    await accountCard.click();

    // Assert detail modal is visible
    const detailModal = page.locator('div[role="dialog"]', { hasText: 'E2E_Tester' }).first();
    await expect(detailModal).toBeVisible({ timeout: 5000 });

    // Close detail modal
    const closeBtn = detailModal.locator('button:has(.lucide-x), button:has-text("Close"), button:has-text("关闭")').first();
    await closeBtn.click();
    await expect(detailModal).not.toBeVisible({ timeout: 5000 });

    // Navigate to Home view to verify route switching works with account state loaded
    const homeNav = page.locator('a[title="主页"], a[title="Home"], a[href="/"]').first();
    await homeNav.click({ timeout: 10000 });
    await expect(page.locator('h1, h2', { hasText: 'Dawnland' }).first()).toBeVisible({ timeout: 10000 });

    // Return to accounts page
    await accountsNav.click({ timeout: 10000 });

    // Hover target card and click delete button
    const targetCard = page.locator('div.border', { hasText: 'E2E_Tester' }).first();
    await targetCard.hover();
    const deleteBtn = targetCard.locator('button[title*="删除"], button[title*="Delete"], button:has(.lucide-trash-2)').first();
    await deleteBtn.click();
    
    // Confirm delete in the dialog
    const confirmBtn = page.locator('button.bg-red-600:has-text("删除"), button.bg-red-600:has-text("Delete")').last();
    await confirmBtn.click();

    // Assert it is gone
    await expect(page.locator('text=E2E_Tester')).toHaveCount(0, { timeout: 5000 });
  });
});

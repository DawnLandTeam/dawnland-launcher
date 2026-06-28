import { test, expect } from './fixtures';

test.describe('Account Management (E2E)', () => {
  test('Add and remove an offline account', async ({ page }) => {
    // Wait for the app to initialize
    await page.waitForTimeout(1000);
    
    // Check if the current URL is loaded
    await page.waitForLoadState('domcontentloaded');

    // Click the Accounts nav item. Find by href or text.
    // Usually it's in the sidebar. Let's just look for an element with text matching account or profile icon.
    // Dawnland Launcher uses Lucide icons and Vue router.
    // If we are already on Home, there is an account switcher at the bottom left or top right.
    const accountLink = page.locator('a[href="/accounts"]');
    if (await accountLink.isVisible()) {
      await accountLink.click();
    } else {
      // Fallback: evaluate router push
      await page.evaluate(() => {
        // @ts-ignore
        window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
        // If we can't find the link, we can just trigger a click on the text that says "Accounts" or similar.
      });
      // Just click the text "Accounts" or Chinese "账号"
      const accountsMenu = page.locator('text=账号管理').first();
      if (await accountsMenu.isVisible()) {
        await accountsMenu.click();
      } else {
        const accountsMenuEn = page.locator('text=Accounts').first();
        if (await accountsMenuEn.isVisible()) await accountsMenuEn.click();
      }
    }

    // Now on Accounts View
    // The "Add Account" button is usually there, or it shows empty state.
    const addAccountBtn = page.locator('button:has-text("添加账号"), button:has-text("Add Account")').first();
    if (await addAccountBtn.isVisible()) {
      await addAccountBtn.click();
    }
    
    // Click the "Offline" type tab
    const offlineTab = page.locator('button:has-text("离线账号"), button:has-text("Offline")').first();
    await offlineTab.click();

    // Type the username
    const usernameInput = page.locator('input[placeholder*="输入用户名"], input[placeholder*="Enter username"]');
    await usernameInput.fill('E2E_Tester');

    // Click Add
    const submitBtn = page.locator('button:has-text("添加"), button:has-text("Add")').last();
    await submitBtn.click();

    // Assert that the account appears in the list
    await expect(page.locator('text=E2E_Tester').first()).toBeVisible({ timeout: 5000 });

    // Click delete button inside the specific account card
    const accountCard = page.locator('div.border.rounded-xl', { hasText: 'E2E_Tester' }).first();
    const deleteBtn = accountCard.locator('button:has-text("删除"), button:has-text("Delete")').first();
    await deleteBtn.click();
    
    // Confirm delete in the dialog (the confirmation button also says "删除" or "Delete")
    const confirmBtn = page.locator('button.bg-red-600:has-text("删除"), button.bg-red-600:has-text("Delete")').last();
    await confirmBtn.click();

    // Assert it is gone (using first() to avoid strict mode violation if the modal is still fading out)
    await expect(page.locator('text=E2E_Tester').first()).not.toBeVisible({ timeout: 5000 });
  });
});

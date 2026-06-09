import { test, expect } from './fixtures';

test.describe('Settings View', () => {
  test('should render and modify settings', async ({ page, mockTauri }) => {
    await mockTauri.setMockResponses({
      'get_settings': {
        java_path: 'C:\\Program Files\\Java\\jdk-17\\bin\\java.exe',
        memory_limit: 4096,
        language: 'en-US'
      },
      'scan_local_javas': [
        { path: 'C:\\Program Files\\Java\\jdk-17\\bin\\java.exe', versionString: '17.0.2', majorVersion: 17, vendor: 'Oracle' },
        { path: 'C:\\Program Files\\Java\\jre1.8.0_202\\bin\\java.exe', versionString: '1.8.0_202', majorVersion: 8, vendor: 'Oracle' }
      ],
      'save_settings': null, // Success
    });

    await page.goto('/settings');

    // Default tab is General, wait for something general
    await expect(page.getByRole('heading', { name: 'Settings', exact: true }).first()).toBeVisible();

    // Go to Java tab
    const javaTab = page.locator('button', { hasText: /Java/i });
    await javaTab.click();

    // Wait for settings to load
    // Check if the Java version is visible
    await expect(page.getByText('17.0.2')).toBeVisible();
    await expect(page.getByText('1.8.0_202').first()).toBeVisible();

    // Check General tab memory slider
    const generalTab = page.locator('button', { hasText: /General|常规/i });
    await generalTab.click();
    
    // The memory slider or input should reflect 4096
    // Check if 4096 is somewhere in the value or text
    await expect(page.getByText(/4096/).first()).toBeVisible();

    // Depending on the exact settings save mechanism, if there's a "Save" button:
    const saveButton = page.locator('button', { hasText: /save|保存/i });
    if (await saveButton.count() > 0) {
      await saveButton.click();
      
      // Verify IPC save_settings was called
      const calls = await mockTauri.getIpcCalls();
      const saveCall = calls.find(c => c.cmd === 'save_settings');
      expect(saveCall).toBeDefined();
    }
  });
});

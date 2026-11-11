import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils';
import ModLoaderInstallDialog from './ModLoaderInstallDialog.vue';
import { invoke } from '@tauri-apps/api/core';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('vue-i18n', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>();
  return {
    ...actual,
    useI18n: () => ({
      t: (key: string, defaultOrParams?: string | Record<string, unknown>) =>
        typeof defaultOrParams === 'string' ? defaultOrParams : key,
    }),
  };
});

vi.mock('../../composables/useToast', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

describe('ModLoaderInstallDialog', () => {
  const wrappers: VueWrapper[] = [];

  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    while (wrappers.length > 0) {
      wrappers.pop()?.unmount();
    }
    document.body.innerHTML = '';
  });

  it('loads forge versions once, matches unprefixed current version, and teleports dropdown outside dialog', async () => {
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === 'get_forge_loaders') {
        return {
          versions: [
            { version: '1.20.1-47.4.29' },
            { version: '1.20.1-47.4.28' },
            { version: '1.20.1-47.4.27' },
          ],
        };
      }
      return [];
    });

    const wrapper = mount(ModLoaderInstallDialog, {
      attachTo: document.body,
      props: {
        open: true,
        instanceId: 'TestForge',
        mcVersion: '1.20.1',
        currentLoaderType: 'Forge',
        currentLoaderVersion: '47.4.28',
        mode: 'switch',
      },
    });
    wrappers.push(wrapper);

    await flushPromises();

    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith('get_forge_loaders', { mcVersion: '1.20.1' });

    const dialog = document.body.querySelector<HTMLElement>('[role="dialog"]');
    expect(dialog).not.toBeNull();
    expect(dialog?.textContent).toContain('Switch Mod Loader Version');
    expect(dialog?.textContent).toContain('FORGE 47.4.28');

    const selectTrigger = dialog?.querySelector<HTMLButtonElement>('button[aria-expanded]');
    expect(selectTrigger?.textContent).toContain('1.20.1-47.4.28 (Current)');

    // Open dropdown and verify it teleports to body outside the dialog scroll container
    selectTrigger?.click();
    await flushPromises();

    const dropdown = document.body.querySelector<HTMLElement>('[data-dselect-dropdown]');
    expect(dropdown).not.toBeNull();
    expect(dialog?.contains(dropdown)).toBe(false);
    expect(dropdown?.textContent).toContain('1.20.1-47.4.29 [Latest]');
    expect(dropdown?.textContent).toContain('1.20.1-47.4.28 (Current)');

    // Select newer version -> Upgrade badge appears
    const optionEls = dropdown?.querySelectorAll<HTMLElement>('[role="option"]') ?? [];
    expect(optionEls.length).toBe(3);
    optionEls[0].click();
    await flushPromises();
    expect(dialog?.textContent).toContain('Upgrade');

    // Re-open dropdown, search for older version, and select -> Downgrade badge appears
    selectTrigger?.click();
    await flushPromises();
    const searchInput = document.body.querySelector<HTMLInputElement>(
      '[data-dselect-dropdown] input[type="text"]',
    );
    expect(searchInput).not.toBeNull();
    if (searchInput) {
      searchInput.value = '47.4.27';
      searchInput.dispatchEvent(new Event('input'));
      await flushPromises();
    }

    const filteredOptions = document.body.querySelectorAll<HTMLElement>(
      '[data-dselect-dropdown] [role="option"]',
    );
    expect(filteredOptions).toHaveLength(1);
    filteredOptions[0].click();
    await flushPromises();
    expect(dialog?.textContent).toContain('Downgrade');
  });

  it('loads versions once when switching loader type in install mode', async () => {
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === 'get_fabric_loaders') {
        return { stable: ['0.16.10'], unstable: [] };
      }
      if (cmd === 'get_neoforge_loaders') {
        return { versions: [{ version: '21.1.72' }] };
      }
      return [];
    });

    const wrapper = mount(ModLoaderInstallDialog, {
      attachTo: document.body,
      props: {
        open: true,
        instanceId: 'VanillaInst',
        mcVersion: '1.21.1',
        mode: 'install',
      },
    });
    wrappers.push(wrapper);

    await flushPromises();
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith('get_fabric_loaders', { mcVersion: '1.21.1' });

    const dialog = document.body.querySelector<HTMLElement>('[role="dialog"]');
    const neoForgeButton = Array.from(dialog?.querySelectorAll('button') ?? []).find((btn) =>
      btn.textContent?.includes('NeoForge'),
    );
    expect(neoForgeButton).toBeDefined();
    neoForgeButton?.click();
    await flushPromises();

    expect(invoke).toHaveBeenCalledTimes(2);
    expect(invoke).toHaveBeenLastCalledWith('get_neoforge_loaders', { mcVersion: '1.21.1' });
  });
});

import { describe, it, expect, afterEach } from 'vitest';
import { mount, type VueWrapper } from '@vue/test-utils';
import DSelect from './DSelect.vue';
import type { SelectOption } from './DSelect.vue';

describe('DSelect', () => {
  const options: SelectOption[] = [
    { label: 'Option 1', value: '1' },
    { label: 'Option 2', value: '2' },
    { label: 'Option 3', value: '3', disabled: true },
  ];

  const wrappers: VueWrapper[] = [];

  function mountSelect(mountOptions: Parameters<typeof mount<typeof DSelect>>[1]) {
    const wrapper = mount(DSelect, {
      attachTo: document.body,
      ...mountOptions,
    });
    wrappers.push(wrapper);
    return wrapper;
  }

  afterEach(() => {
    while (wrappers.length > 0) {
      wrappers.pop()?.unmount();
    }
    document.body.innerHTML = '';
  });

  it('renders placeholder when no modelValue is selected', () => {
    const wrapper = mountSelect({
      props: {
        options,
        placeholder: 'Select an option',
      },
    });

    expect(wrapper.text()).toContain('Select an option');
  });

  it('renders selected label when modelValue is provided', () => {
    const wrapper = mountSelect({
      props: {
        options,
        modelValue: '2',
      },
    });

    expect(wrapper.text()).toContain('Option 2');
  });

  it('teleports dropdown to body when clicked', async () => {
    const wrapper = mountSelect({
      props: { options },
    });

    const button = wrapper.find('button');
    await button.trigger('click');

    const dropdown = document.body.querySelector('[data-dselect-dropdown]');
    expect(dropdown).not.toBeNull();
    expect(wrapper.element.contains(dropdown)).toBe(false);
    expect(dropdown?.textContent).toContain('Option 1');
    expect(dropdown?.textContent).toContain('Option 2');
  });

  it('emits update:modelValue when an option is selected', async () => {
    const wrapper = mountSelect({
      props: { options },
    });

    await wrapper.find('button').trigger('click');

    const optionElements = document.body.querySelectorAll<HTMLElement>(
      '[data-dselect-dropdown] [role="option"]:not([aria-disabled="true"])',
    );
    expect(optionElements.length).toBeGreaterThan(0);
    optionElements[0].click();
    await wrapper.vm.$nextTick();

    expect(wrapper.emitted('update:modelValue')).toBeTruthy();
    expect(wrapper.emitted('update:modelValue')?.[0]).toEqual(['1']);
    expect(document.body.querySelector('[data-dselect-dropdown]')).toBeNull();
  });

  it('does not emit update:modelValue for disabled options', async () => {
    const wrapper = mountSelect({
      props: { options },
    });

    await wrapper.find('button').trigger('click');

    const disabledOption = document.body.querySelector<HTMLElement>(
      '[data-dselect-dropdown] [role="option"][aria-disabled="true"]',
    );
    disabledOption?.click();
    await wrapper.vm.$nextTick();

    expect(wrapper.emitted('update:modelValue')).toBeFalsy();
  });

  it('filters options by keyword when searchable is enabled and renders instance icons', async () => {
    const instanceOptions: SelectOption[] = [
      {
        label: 'All the Mods 11',
        sublabel: '1.21.1 · NeoForge',
        value: 'atm11',
        iconPath: null,
        showInstanceIcon: true,
      },
      {
        label: 'Fabric Survival',
        sublabel: '1.20.1 · Fabric',
        value: 'fabric-1.20.1',
        iconPath: null,
        showInstanceIcon: true,
      },
    ];

    const wrapper = mountSelect({
      props: {
        options: instanceOptions,
        modelValue: 'atm11',
        searchable: true,
        searchPlaceholder: 'Search instances...',
      },
    });

    expect(wrapper.find('button img').exists()).toBe(true);
    expect(wrapper.find('button').text()).toContain('All the Mods 11');
    expect(wrapper.find('button').text()).toContain('1.21.1 · NeoForge');

    await wrapper.find('button').trigger('click');
    const searchInput = document.body.querySelector<HTMLInputElement>(
      '[data-dselect-dropdown] input[type="text"]',
    );
    expect(searchInput).not.toBeNull();

    if (searchInput) {
      searchInput.value = 'fabric';
      searchInput.dispatchEvent(new Event('input'));
      await wrapper.vm.$nextTick();
    }

    const visibleOptions = document.body.querySelectorAll('[data-dselect-dropdown] [role="option"]');
    expect(visibleOptions).toHaveLength(1);
    expect(visibleOptions[0].textContent).toContain('Fabric Survival');
  });

  it('keeps dropdown open on inner click and closes on outside click', async () => {
    const wrapper = mountSelect({
      props: {
        options,
        searchable: true,
      },
    });

    await wrapper.find('button').trigger('click');
    const dropdown = document.body.querySelector<HTMLElement>('[data-dselect-dropdown]');
    const searchInput = dropdown?.querySelector<HTMLInputElement>('input[type="text"]');
    expect(dropdown).not.toBeNull();
    expect(searchInput).not.toBeNull();

    // Pointer interaction inside teleported dropdown should not close it
    searchInput?.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }));
    searchInput?.dispatchEvent(new MouseEvent('click', { bubbles: true, detail: 1 }));
    await wrapper.vm.$nextTick();
    expect(document.body.querySelector('[data-dselect-dropdown]')).not.toBeNull();

    // VueUse onClickOutside guards click handling per macrotask tick via setTimeout(..., 0)
    await new Promise((resolve) => setTimeout(resolve, 0));

    // Pointer interaction outside both trigger and dropdown should close it
    document.body.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }));
    document.body.dispatchEvent(new MouseEvent('click', { bubbles: true, detail: 1 }));
    await wrapper.vm.$nextTick();
    expect(document.body.querySelector('[data-dselect-dropdown]')).toBeNull();
  });

  it('flips dropdown upward when viewport space below is insufficient', async () => {
    const wrapper = mountSelect({
      props: { options },
    });

    const originalInnerHeight = window.innerHeight;
    Object.defineProperty(window, 'innerHeight', { configurable: true, value: 600 });

    const triggerButton = wrapper.find('button').element as HTMLButtonElement;
    triggerButton.getBoundingClientRect = () =>
      ({
        top: 520,
        bottom: 560,
        left: 100,
        right: 340,
        width: 240,
        height: 40,
        x: 100,
        y: 520,
        toJSON: () => ({}),
      }) as DOMRect;

    try {
      await wrapper.find('button').trigger('click');

      const dropdown = document.body.querySelector<HTMLElement>('[data-dselect-dropdown]');
      expect(dropdown).not.toBeNull();
      expect(dropdown?.className).toContain('origin-bottom');
      expect(dropdown?.style.bottom).toBe('84px');
      expect(dropdown?.style.width).toBe('240px');
    } finally {
      Object.defineProperty(window, 'innerHeight', {
        configurable: true,
        value: originalInnerHeight,
      });
    }
  });
});

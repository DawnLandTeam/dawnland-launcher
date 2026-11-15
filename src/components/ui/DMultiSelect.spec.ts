import { describe, it, expect, afterEach } from 'vitest';
import { mount, type VueWrapper } from '@vue/test-utils';
import DMultiSelect from './DMultiSelect.vue';
import type { SelectOption } from './DMultiSelect.vue';

describe('DMultiSelect', () => {
  const options: SelectOption[] = [
    { label: 'Option 1', value: '1' },
    { label: 'Option 2', value: '2' },
    { label: 'Option 3', value: '3', disabled: true },
  ];

  const globalMountOptions = {
    mocks: {
      $t: (msg: string) => msg,
    },
  };

  const wrappers: VueWrapper[] = [];

  function mountMultiSelect(mountOptions: Parameters<typeof mount<typeof DMultiSelect>>[1]) {
    const wrapper = mount(DMultiSelect, {
      attachTo: document.body,
      ...mountOptions,
      global: {
        ...globalMountOptions,
        ...mountOptions?.global,
      },
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
    const wrapper = mountMultiSelect({
      props: {
        options,
        modelValue: [],
        placeholder: 'Select options',
      },
    });

    expect(wrapper.text()).toContain('Select options');
  });

  it('renders selected labels when modelValue has items', () => {
    const wrapper = mountMultiSelect({
      props: {
        options,
        modelValue: ['1', '2'],
      },
    });

    expect(wrapper.text()).toContain('Option 1, Option 2');
  });

  it('teleports dropdown to body when clicked', async () => {
    const wrapper = mountMultiSelect({
      props: { options, modelValue: [] },
    });

    const button = wrapper.find('button');
    await button.trigger('click');

    const dropdown = document.body.querySelector('[data-dmultiselect-dropdown]');
    expect(dropdown).not.toBeNull();
    expect(wrapper.element.contains(dropdown)).toBe(false);
    expect(dropdown?.textContent).toContain('Option 1');
    expect(dropdown?.textContent).toContain('Option 2');
  });

  it('emits update:modelValue with toggled items', async () => {
    const wrapper = mountMultiSelect({
      props: { options, modelValue: ['1'] },
    });

    await wrapper.find('button').trigger('click');

    const optionElements = document.body.querySelectorAll<HTMLElement>(
      '[data-dmultiselect-dropdown] [role="option"]:not([aria-disabled="true"])',
    );
    optionElements[1].click();
    await wrapper.vm.$nextTick();

    expect(wrapper.emitted('update:modelValue')).toBeTruthy();
    expect(wrapper.emitted('update:modelValue')?.[0]).toEqual([['1', '2']]);

    optionElements[0].click();
    await wrapper.vm.$nextTick();
    expect(wrapper.emitted('update:modelValue')?.[1]).toEqual([[]]);
  });

  it('filters options based on search query', async () => {
    const wrapper = mountMultiSelect({
      props: { options, modelValue: [] },
    });

    await wrapper.find('button').trigger('click');

    const searchInput = document.body.querySelector<HTMLInputElement>(
      '[data-dmultiselect-dropdown] input[type="text"]',
    );
    expect(searchInput).not.toBeNull();
    if (searchInput) {
      searchInput.value = 'Option 1';
      searchInput.dispatchEvent(new Event('input'));
    }
    await wrapper.vm.$nextTick();

    const dropdown = document.body.querySelector('[data-dmultiselect-dropdown]');
    expect(dropdown?.textContent).toContain('Option 1');
    expect(dropdown?.textContent).not.toContain('Option 2');
  });
});

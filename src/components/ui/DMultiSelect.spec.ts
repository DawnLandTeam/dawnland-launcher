import { describe, it, expect } from 'vitest';
import { mount } from '@vue/test-utils';
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
      $t: (msg: string) => msg
    }
  };

  it('renders placeholder when no modelValue is selected', () => {
    const wrapper = mount(DMultiSelect, {
      props: {
        options,
        modelValue: [],
        placeholder: 'Select options'
      },
      global: globalMountOptions
    });
    
    expect(wrapper.text()).toContain('Select options');
  });

  it('renders selected labels when modelValue has items', () => {
    const wrapper = mount(DMultiSelect, {
      props: {
        options,
        modelValue: ['1', '2']
      },
      global: globalMountOptions
    });
    
    expect(wrapper.text()).toContain('Option 1, Option 2');
  });

  it('opens dropdown when clicked', async () => {
    const wrapper = mount(DMultiSelect, {
      props: { options, modelValue: [] },
      global: globalMountOptions
    });
    
    const button = wrapper.find('button');
    await button.trigger('click');
    
    expect(wrapper.text()).toContain('Option 1');
    expect(wrapper.text()).toContain('Option 2');
  });

  it('emits update:modelValue with toggled items', async () => {
    const wrapper = mount(DMultiSelect, {
      props: { options, modelValue: ['1'] },
      global: globalMountOptions
    });
    
    await wrapper.find('button').trigger('click');
    
    // Click Option 2 to add it
    const optionElements = wrapper.findAll('[role="option"]:not([aria-disabled="true"])');
    await optionElements[1].trigger('click'); // Option 2
    
    expect(wrapper.emitted('update:modelValue')).toBeTruthy();
    expect(wrapper.emitted('update:modelValue')?.[0]).toEqual([['1', '2']]);
    
    // Click Option 1 to remove it
    await optionElements[0].trigger('click'); // Option 1
    expect(wrapper.emitted('update:modelValue')?.[1]).toEqual([[]]);
  });

  it('filters options based on search query', async () => {
    const wrapper = mount(DMultiSelect, {
      props: { options, modelValue: [] },
      global: globalMountOptions
    });
    
    await wrapper.find('button').trigger('click');
    
    const searchInput = wrapper.find('input[type="text"]');
    await searchInput.setValue('Option 1');
    
    expect(wrapper.text()).toContain('Option 1');
    expect(wrapper.text()).not.toContain('Option 2');
  });
});

import { describe, it, expect } from 'vitest';
import { mount } from '@vue/test-utils';
import DSelect from './DSelect.vue';
import type { SelectOption } from './DSelect.vue';

describe('DSelect', () => {
  const options: SelectOption[] = [
    { label: 'Option 1', value: '1' },
    { label: 'Option 2', value: '2' },
    { label: 'Option 3', value: '3', disabled: true },
  ];

  it('renders placeholder when no modelValue is selected', () => {
    const wrapper = mount(DSelect, {
      props: {
        options,
        placeholder: 'Select an option'
      }
    });
    
    expect(wrapper.text()).toContain('Select an option');
  });

  it('renders selected label when modelValue is provided', () => {
    const wrapper = mount(DSelect, {
      props: {
        options,
        modelValue: '2'
      }
    });
    
    expect(wrapper.text()).toContain('Option 2');
  });

  it('opens dropdown when clicked', async () => {
    const wrapper = mount(DSelect, {
      props: { options }
    });
    
    const button = wrapper.find('button');
    await button.trigger('click');
    
    // After click, options should be visible
    expect(wrapper.text()).toContain('Option 1');
    expect(wrapper.text()).toContain('Option 2');
  });

  it('emits update:modelValue when an option is selected', async () => {
    const wrapper = mount(DSelect, {
      props: { options }
    });
    
    await wrapper.find('button').trigger('click');
    
    // Find all options in the dropdown and click the first one
    const optionElements = wrapper.findAll('.cursor-pointer');
    await optionElements[0].trigger('click');
    
    expect(wrapper.emitted('update:modelValue')).toBeTruthy();
    expect(wrapper.emitted('update:modelValue')?.[0]).toEqual(['1']);
  });

  it('does not emit update:modelValue for disabled options', async () => {
    const wrapper = mount(DSelect, {
      props: { options }
    });
    
    await wrapper.find('button').trigger('click');
    
    // The disabled option has cursor-not-allowed class
    const disabledOption = wrapper.find('.cursor-not-allowed');
    await disabledOption.trigger('click');
    
    expect(wrapper.emitted('update:modelValue')).toBeFalsy();
  });
});

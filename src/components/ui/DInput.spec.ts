import { describe, it, expect } from 'vitest';
import { mount } from '@vue/test-utils';
import DInput from './DInput.vue';

describe('DInput', () => {
  it('renders correctly with modelValue', () => {
    const wrapper = mount(DInput, {
      props: {
        modelValue: 'Initial Text'
      }
    });
    
    const input = wrapper.find('input');
    expect(input.element.value).toBe('Initial Text');
  });

  it('emits update:modelValue on input', async () => {
    const wrapper = mount(DInput, {
      props: {
        modelValue: ''
      }
    });
    
    const input = wrapper.find('input');
    await input.setValue('New Value');
    
    expect(wrapper.emitted()).toHaveProperty('update:modelValue');
    expect(wrapper.emitted('update:modelValue')?.[0]).toEqual(['New Value']);
  });

  it('disables the input when disabled prop is true via attrs', () => {
    const wrapper = mount(DInput, {
      attrs: {
        disabled: true
      }
    });
    
    const input = wrapper.find('input');
    expect(input.element.disabled).toBe(true);
  });
});

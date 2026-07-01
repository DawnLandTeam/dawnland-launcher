import { describe, it, expect } from 'vitest';
import { mount } from '@vue/test-utils';
import DToggleGroup from './DToggleGroup.vue';
import type { ToggleOption } from './DToggleGroup.vue';

describe('DToggleGroup', () => {
  const options: ToggleOption[] = [
    { label: 'Left', value: 'left' },
    { label: 'Right', value: 'right' }
  ];

  it('renders options correctly', () => {
    const wrapper = mount(DToggleGroup, {
      props: {
        options,
        modelValue: 'left'
      }
    });
    
    expect(wrapper.text()).toContain('Left');
    expect(wrapper.text()).toContain('Right');
  });

  it('applies active styling to the selected option', () => {
    const wrapper = mount(DToggleGroup, {
      props: {
        options,
        modelValue: 'left'
      }
    });
    
    const buttons = wrapper.findAll('button');
    expect(buttons[0].classes()).toContain('text-emerald-600');
    expect(buttons[1].classes()).toContain('text-gray-500');
  });

  it('emits update:modelValue when an option is clicked', async () => {
    const wrapper = mount(DToggleGroup, {
      props: {
        options,
        modelValue: 'left'
      }
    });
    
    const buttons = wrapper.findAll('button');
    await buttons[1].trigger('click');
    
    expect(wrapper.emitted('update:modelValue')).toBeTruthy();
    expect(wrapper.emitted('update:modelValue')?.[0]).toEqual(['right']);
  });
});

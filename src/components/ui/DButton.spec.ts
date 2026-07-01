import { describe, it, expect } from 'vitest';
import { mount } from '@vue/test-utils';
import DButton from './DButton.vue';

describe('DButton', () => {
  it('renders slot content correctly', () => {
    const wrapper = mount(DButton, {
      slots: {
        default: 'Click Me'
      }
    });
    
    expect(wrapper.text()).toContain('Click Me');
  });

  it('handles click events', async () => {
    const wrapper = mount(DButton);
    
    await wrapper.trigger('click');
    
    expect(wrapper.emitted()).toHaveProperty('click');
    expect(wrapper.emitted('click')).toHaveLength(1);
  });

  it('disables the button when disabled prop is true', async () => {
    const wrapper = mount(DButton, {
      props: {
        disabled: true
      }
    });
    
    const button = wrapper.find('button');
    expect(button.element.disabled).toBe(true);
  });
});

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { mount } from '@vue/test-utils';
import DTooltip from './DTooltip.vue';

describe('DTooltip', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
    document.body.innerHTML = '';
  });

  it('renders trigger slot correctly', () => {
    const wrapper = mount(DTooltip, {
      props: {
        content: 'Tooltip text'
      },
      slots: {
        default: '<button class="trigger-btn">Hover me</button>'
      }
    });

    expect(wrapper.find('.trigger-btn').exists()).toBe(true);
    expect(wrapper.find('.trigger-btn').text()).toBe('Hover me');
  });

  it('shows tooltip content on mouseenter after delay and hides on mouseleave', async () => {
    const wrapper = mount(DTooltip, {
      props: {
        content: 'Detailed explanation text',
        delay: 50
      },
      slots: {
        default: 'Help icon'
      },
      attachTo: document.body
    });

    const triggerSpan = wrapper.find('span.inline-flex');
    expect(document.querySelector('[role="tooltip"]')).toBeNull();

    await triggerSpan.trigger('mouseenter');
    // Before delay, it should not be shown yet
    expect(document.querySelector('[role="tooltip"]')).toBeNull();

    // Advance time past delay
    await vi.advanceTimersByTimeAsync(60);
    await wrapper.vm.$nextTick();

    const tooltip = document.querySelector('[role="tooltip"]');
    expect(tooltip).not.toBeNull();
    expect(tooltip?.textContent).toContain('Detailed explanation text');

    // Trigger mouseleave
    await triggerSpan.trigger('mouseleave');
    await wrapper.vm.$nextTick();

    expect(document.querySelector('[role="tooltip"]')).toBeNull();
  });

  it('does not show tooltip when disabled is true', async () => {
    const wrapper = mount(DTooltip, {
      props: {
        content: 'Disabled tooltip',
        disabled: true,
        delay: 0
      },
      slots: {
        default: 'Help icon'
      },
      attachTo: document.body
    });

    const triggerSpan = wrapper.find('span.inline-flex');
    await triggerSpan.trigger('mouseenter');
    await vi.advanceTimersByTimeAsync(10);
    await wrapper.vm.$nextTick();

    expect(document.querySelector('[role="tooltip"]')).toBeNull();
  });

  it('supports custom content slot', async () => {
    const wrapper = mount(DTooltip, {
      props: {
        delay: 0
      },
      slots: {
        default: 'Trigger',
        content: '<span class="custom-content">Custom slot content</span>'
      },
      attachTo: document.body
    });

    const triggerSpan = wrapper.find('span.inline-flex');
    await triggerSpan.trigger('mouseenter');
    await vi.advanceTimersByTimeAsync(10);
    await wrapper.vm.$nextTick();

    const tooltip = document.querySelector('[role="tooltip"]');
    expect(tooltip).not.toBeNull();
    expect(tooltip?.querySelector('.custom-content')?.textContent).toBe('Custom slot content');
  });
});

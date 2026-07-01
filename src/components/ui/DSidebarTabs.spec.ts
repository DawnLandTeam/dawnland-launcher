import { describe, it, expect, vi } from 'vitest';
import { mount } from '@vue/test-utils';
import DSidebarTabs from './DSidebarTabs.vue';
import type { SidebarTab } from './DSidebarTabs.vue';

describe('DSidebarTabs', () => {
  const getTabs = (): SidebarTab[] => [
    { id: 'tab1', name: 'Tab 1' },
    { id: 'tab2', name: 'Tab 2' },
    { id: 'tab3', name: 'Tab 3', disabled: true },
    { id: 'tab4', name: 'Tab 4', action: vi.fn() }
  ];

  it('renders title and tabs correctly', () => {
    const wrapper = mount(DSidebarTabs, {
      props: {
        title: 'Settings',
        tabs: getTabs(),
        modelValue: 'tab1'
      }
    });
    
    expect(wrapper.text()).toContain('Settings');
    expect(wrapper.text()).toContain('Tab 1');
    expect(wrapper.text()).toContain('Tab 2');
  });

  it('applies active styles to the selected tab', () => {
    const wrapper = mount(DSidebarTabs, {
      props: {
        tabs: getTabs(),
        modelValue: 'tab1'
      }
    });
    
    const activeTab = wrapper.find('[role="tab"][aria-selected="true"]');
    expect(activeTab.exists()).toBe(true);
    expect(activeTab.text()).toContain('Tab 1');
  });

  it('emits update:modelValue when a normal tab is clicked', async () => {
    const wrapper = mount(DSidebarTabs, {
      props: {
        tabs: getTabs(),
        modelValue: 'tab1'
      }
    });
    
    const buttons = wrapper.findAll('button');
    await buttons[1].trigger('click'); // Click Tab 2
    
    expect(wrapper.emitted('update:modelValue')).toBeTruthy();
    expect(wrapper.emitted('update:modelValue')?.[0]).toEqual(['tab2']);
  });

  it('does not emit for disabled tabs', async () => {
    const wrapper = mount(DSidebarTabs, {
      props: {
        tabs: getTabs(),
        modelValue: 'tab1'
      }
    });
    
    const buttons = wrapper.findAll('button');
    await buttons[2].trigger('click'); // Click Tab 3 (disabled)
    
    expect(wrapper.emitted('update:modelValue')).toBeFalsy();
  });

  it('calls action instead of emitting if action is defined', async () => {
    const tabs = getTabs();
    const wrapper = mount(DSidebarTabs, {
      props: {
        tabs,
        modelValue: 'tab1'
      }
    });
    
    const buttons = wrapper.findAll('button');
    await buttons[3].trigger('click'); // Click Tab 4
    
    expect(tabs[3].action).toHaveBeenCalled();
    expect(wrapper.emitted('update:modelValue')).toBeFalsy();
  });
});

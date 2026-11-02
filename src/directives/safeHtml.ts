import { Directive } from 'vue';
import DOMPurify from 'dompurify';
import { openUrl } from '@tauri-apps/plugin-opener';

const safeConfig = {
  ALLOWED_TAGS: ['h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'blockquote', 'p', 'a', 'ul', 'ol',
    'nl', 'li', 'b', 'i', 'strong', 'em', 'strike', 'code', 'hr', 'br', 'div',
    'table', 'thead', 'caption', 'tbody', 'tr', 'th', 'td', 'pre', 'img'],
  ALLOWED_ATTR: ['href', 'name', 'target', 'src', 'alt', 'class'],
  RETURN_DOM_FRAGMENT: true
};

function handleSafeLinkClick(e: MouseEvent) {
  const target = e.target as HTMLElement | null;
  const anchor = target?.closest('a');
  if (anchor) {
    e.preventDefault();
    const href = anchor.getAttribute('href')?.trim();
    if (href && (href.startsWith('https://') || href.startsWith('http://'))) {
      openUrl(href).catch((err) => console.error('Failed to open external URL:', err));
    }
  }
}

export const safeHtml: Directive = {
  mounted(el, binding) {
    const fragment = DOMPurify.sanitize(binding.value, safeConfig) as unknown as Node;
    el.replaceChildren(fragment);
    el.addEventListener('click', handleSafeLinkClick);
  },
  updated(el, binding) {
    if (binding.value !== binding.oldValue) {
      const fragment = DOMPurify.sanitize(binding.value, safeConfig) as unknown as Node;
      el.replaceChildren(fragment);
    }
  },
  unmounted(el) {
    el.removeEventListener('click', handleSafeLinkClick);
  }
};

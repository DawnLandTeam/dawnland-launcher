import { Directive } from 'vue';
import DOMPurify from 'dompurify';

// Configure DOMPurify to allow standard markdown HTML elements globally
DOMPurify.setConfig({
  ALLOWED_TAGS: ['h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'blockquote', 'p', 'a', 'ul', 'ol',
    'nl', 'li', 'b', 'i', 'strong', 'em', 'strike', 'code', 'hr', 'br', 'div',
    'table', 'thead', 'caption', 'tbody', 'tr', 'th', 'td', 'pre', 'img'],
  ALLOWED_ATTR: ['href', 'name', 'target', 'src', 'alt', 'class']
});

export const safeHtml: Directive = {
  mounted(el, binding) {
    const fragment = DOMPurify.sanitize(binding.value, { RETURN_DOM_FRAGMENT: true }) as unknown as Node;
    el.replaceChildren(fragment);
  },
  updated(el, binding) {
    if (binding.value !== binding.oldValue) {
      const fragment = DOMPurify.sanitize(binding.value, { RETURN_DOM_FRAGMENT: true }) as unknown as Node;
      el.replaceChildren(fragment);
    }
  }
};

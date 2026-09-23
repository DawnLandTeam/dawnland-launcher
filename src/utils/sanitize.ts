import type { Config } from 'dompurify';

export const sanitizeOptions: Config = {
  ALLOWED_TAGS: ['h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'p', 'br', 'ul', 'ol', 'li', 'strong', 'em', 'b', 'i', 'del', 'blockquote', 'code', 'pre', 'a', 'img', 'table', 'thead', 'tbody', 'tr', 'th', 'td', 'hr', 'span', 'div', 'input'],
  ALLOWED_ATTR: ['href', 'title', 'alt', 'src', 'class', 'target', 'rel', 'type', 'checked', 'disabled']
};

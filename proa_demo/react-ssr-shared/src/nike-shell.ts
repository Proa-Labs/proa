// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { NIKE_CSS } from "./nike-css";

export const NIKE_CATALOG_TITLE = "Men's Shoes & Sneakers. Nike.com";
export const NIKE_PDP_TITLE = "Nike Alphafly 3. Nike.com";

const NIKE_CATALOG_DESCRIPTION =
  "Shop Men's shoes and sneakers at Nike.com. Browse the latest styles, filter by sport, price, and more.";
const NIKE_PDP_DESCRIPTION =
  "Shop the Nike Alphafly 3 men's road racing shoes. Fine-tuned for marathon speed with ZoomX foam, Air Zoom units, and a full-length carbon fiber plate.";

const DOCUMENT_END = `<script>
// Collapsible filter groups
document.querySelectorAll('.nike-filter-group__title').forEach(t => {
  if (!t.nextElementSibling) return;
  t.style.cursor = 'pointer';
  t.addEventListener('click', () => {
    const g = t.closest('.nike-filter-group');
    g.classList.toggle('collapsed');
  });
});
// Checkbox toggles
document.querySelectorAll('.nike-filter-item').forEach(item => {
  item.addEventListener('click', e => {
    e.preventDefault();
    const cb = item.querySelector('.nike-checkbox');
    cb.classList.toggle('is-checked');
  });
});
</script></body></html>`;

export function wrapInNikeShell(title: string, bodyHtml: string): string {
  const description = title === NIKE_PDP_TITLE ? NIKE_PDP_DESCRIPTION : NIKE_CATALOG_DESCRIPTION;

  return `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8"/>
<meta name="viewport" content="width=device-width, initial-scale=1.0"/>
<title>${title}</title>
<meta name="description" content="${description}"/>
<style>${NIKE_CSS}</style>
</head><body class="nike-page">${bodyHtml}${DOCUMENT_END}`;
}

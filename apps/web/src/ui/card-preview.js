import { assetImage } from '../assets/index.js?v=1';
import { attributeSwatch } from './attribute.js?v=1';

export function cardPreviewContent({
  id,
  name,
  rarity,
  attribute,
  imageUrls,
}) {
  const content = document.createElement('span');
  content.className = 'card-preview-content';

  const visual = document.createElement('span');
  visual.className = 'card-preview-visual';

  const attributeNode = document.createElement('span');
  attributeNode.className = 'card-preview-attribute';
  attributeNode.append(attributeSwatch(attribute));

  const icon = assetImage(imageUrls, 'card-preview-icon', name);
  visual.append(attributeNode);
  if (icon) {
    visual.append(icon);
  }

  const nameNode = document.createElement('span');
  nameNode.className = 'card-preview-name';
  nameNode.textContent = name;

  const meta = document.createElement('span');
  meta.className = 'card-preview-meta';
  meta.textContent = `ID: ${id} / ${rarity || '-'}星`;

  content.append(visual, nameNode, meta);
  return content;
}

export function cardPreviewItem({
  id,
  name,
  rarity,
  attribute,
  imageUrls,
  className,
  title,
  selected = false,
  interactive = false,
  leading,
}) {
  const item = document.createElement('div');
  item.className = ['card-preview-item', className].filter(Boolean).join(' ');
  item.classList.toggle('is-selected', selected);
  if (title) {
    item.title = title;
  }
  if (interactive) {
    item.role = 'button';
    item.tabIndex = 0;
    item.dataset.cardId = id;
  }
  if (leading) {
    item.append(leading);
  }
  item.append(cardPreviewContent({
    id,
    name,
    rarity,
    attribute,
    imageUrls,
  }));
  return item;
}

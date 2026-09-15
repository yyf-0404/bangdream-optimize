import { cardIconUrls } from '../../assets/index.js';
import { cardGrowth } from './growth.js';
export function imageSources(card) {
  if (card.custom) return [card.image || new URL('../../../assets/brand.svg', import.meta.url).href];
  return cardIconUrls({ cardId: card.id, card: card.record, illustTrainingStatus: cardGrowth(card).illustTrained });
}
export function setCardImage(image, card) {
  const urls = imageSources(card); let index = 0;
  const fallback = image.parentElement.querySelector('.image-fallback');
  image.hidden = !urls.length;
  if (fallback) fallback.hidden = !!urls.length;
  image.onerror = () => {
    if (++index < urls.length) image.src = urls[index];
    else { image.hidden = true; if (fallback) fallback.hidden = false; }
  };
  if (urls.length) image.src = urls[0];
}

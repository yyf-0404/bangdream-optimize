import { compactJoin } from '../utils.js?v=1';

export function attributeSwatch(attribute) {
  const swatch = document.createElement('span');
  swatch.className = compactJoin(['attribute-swatch', attribute && `attribute-${attribute}`]);
  return swatch;
}

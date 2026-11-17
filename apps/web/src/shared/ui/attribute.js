import { compactJoin } from '../utils.js';

export function attributeSwatch(attribute) {
  const swatch = document.createElement('span');
  swatch.className = compactJoin(['attribute-swatch', attribute && `attribute-${attribute}`]);
  return swatch;
}

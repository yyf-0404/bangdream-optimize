const SVG_NS = 'http://www.w3.org/2000/svg';

const BUTTON_ICON_PATHS = {
  trash:
    'M9 3a1 1 0 0 0-1 1v1H5a1 1 0 1 0 0 2h14a1 1 0 1 0 0-2h-3V4a1 1 0 0 0-1-1H9Zm1 2h4V5h-4Zm-3 4a1 1 0 0 0-1 1v8.5A2.5 2.5 0 0 0 8.5 21h7a2.5 2.5 0 0 0 2.5-2.5V10a1 1 0 1 0-2 0v8.5a.5.5 0 0 1-.5.5h-7a.5.5 0 0 1-.5-.5V10a1 1 0 0 0-1-1Z',
  chevronDown:
    'M6.3 9.3a1 1 0 0 1 1.4 0L12 13.6l4.3-4.3a1 1 0 1 1 1.4 1.4l-5 5a1 1 0 0 1-1.4 0l-5-5a1 1 0 0 1 0-1.4Z',
};

export function buttonIcon(name, className = 'button-icon') {
  const pathData = BUTTON_ICON_PATHS[name];
  if (!pathData) {
    throw new Error(`Unknown button icon: ${name}`);
  }

  const svg = document.createElementNS(SVG_NS, 'svg');
  svg.setAttribute('class', className);
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('aria-hidden', 'true');

  const path = document.createElementNS(SVG_NS, 'path');
  path.setAttribute('d', pathData);
  svg.append(path);
  return svg;
}

export function iconButton({
  icon,
  label,
  className = 'compact-button',
  title,
  onClick,
}) {
  const button = document.createElement('button');
  button.type = 'button';
  button.className = className;
  button.setAttribute('aria-label', label);
  if (title) {
    button.title = title;
  }
  button.append(buttonIcon(icon));
  if (typeof onClick === 'function') {
    button.addEventListener('click', onClick);
  }
  return button;
}

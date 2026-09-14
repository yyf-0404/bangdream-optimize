"""Render the shared sidebar SVG into web/desktop icons (requires CairoSVG and Pillow)."""
from io import BytesIO
from pathlib import Path

import cairosvg
from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
svg = ROOT / 'apps/web/assets/brand.svg'
art = Image.open(BytesIO(cairosvg.svg2png(url=str(svg), output_height=960))).convert('RGBA')
canvas = Image.new('RGBA', (1024, 1024))
canvas.alpha_composite(art, ((1024 - art.width) // 2, (1024 - art.height) // 2))
png = canvas.resize((512, 512), Image.Resampling.LANCZOS)
for relative in ['apps/web/icon.png', 'apps/desktop/src-tauri/icons/icon.png']:
    png.save(ROOT / relative)
png.save(ROOT / 'apps/desktop/src-tauri/icons/icon.ico', sizes=[(n, n) for n in [16, 24, 32, 48, 64, 128, 256]])
print('Updated web PNG, desktop PNG and multi-size ICO from the shared brand SVG.')

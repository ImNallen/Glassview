"""Render the Glassview logo set. Requires rsvg-convert and Python Pillow."""

from io import BytesIO
from pathlib import Path
import shutil
import subprocess

from PIL import Image, ImageDraw, ImageFont


ROOT = Path(__file__).resolve().parents[1]
BRAND = ROOT / "assets" / "brand"
CYAN = "#4FC5D5"
INK = "#0E2F38"
MIST = "#F4F8F9"
SLATE = "#C3CDD0"
SLATE_INK = "#3B4A4F"

DOT = '<circle cx="12" cy="12" r="2.5" fill="currentColor"/>'
INNER = '<circle cx="12" cy="12" r="5.75" stroke="currentColor" stroke-width="2"/>'
OUTER = '<circle cx="12" cy="12" r="10" stroke="currentColor" stroke-width="1.5"/>'
MARK = f'{DOT}\n{INNER}\n<g opacity="0.55">{OUTER}</g>'
# The off state keeps the full silhouette so the tray item doesn't change size, dims it
# so a monochrome menu bar reads it as disabled, and cuts a gap around the slash so the
# slash stays separate from the rings at 18 pt.
OFF_MARK = (
    '<mask id="slash"><rect width="24" height="24" fill="#FFFFFF"/>'
    '<path d="M3 3L21 21" stroke="#000000" stroke-width="4.5" stroke-linecap="round"/></mask>\n'
    f'<g mask="url(#slash)" opacity="0.5">\n{DOT}\n{INNER}\n{OUTER}\n</g>\n'
    '<path d="M3 3L21 21" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>'
)


def svg(content, viewbox="0 0 24 24"):
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{viewbox}" fill="none">\n'
        f"<title>Glassview</title>\n{content}\n</svg>\n"
    )


def render(document, size):
    output = subprocess.run(
        ["rsvg-convert", "--width", str(size), "--height", str(size)],
        input=document.encode(), capture_output=True, check=True,
    ).stdout
    return Image.open(BytesIO(output)).convert("RGBA")


def tile(mark, background, foreground, inset=64, margin=96):
    side = 1024 - 2 * inset
    offset = inset + margin
    return svg(
        f'<rect x="{inset}" y="{inset}" width="{side}" height="{side}" rx="{side * 25 / 112:g}" fill="{background}"/>\n'
        f'<g transform="translate({offset} {offset}) scale({(1024 - 2 * offset) / 24:.10g})">\n'
        + mark.replace("currentColor", foreground) + "\n</g>",
        "0 0 1024 1024",
    )


def save_variant(name, color):
    document = svg(MARK.replace("currentColor", color))
    (BRAND / f"{name}.svg").write_text(document)
    return document


BRAND.mkdir(parents=True, exist_ok=True)
(BRAND / "mark.svg").write_text(svg(MARK))
dark = save_variant("mark-dark", INK)
white = save_variant("mark-white", "#FFFFFF")
accent = save_variant("mark-accent", CYAN)
black = svg(MARK.replace("currentColor", "#000000"))
black_off = svg(OFF_MARK.replace("currentColor", "#000000"))

for size in (16, 22, 32, 44):
    render(black, size).save(BRAND / f"menu-bar-{size}.png")

app = tile(MARK, CYAN, INK)
# Tray icons fill their whole canvas because Windows shows them at 16 px.
tray_on = tile(MARK, CYAN, INK, inset=0, margin=64)
tray_off = tile(OFF_MARK, SLATE, SLATE_INK, inset=0, margin=64)
avatar = svg(
    f'<path fill="{CYAN}" d="M0 0H1024V1024H0Z"/>\n'
    f'<g transform="translate(160 160) scale(29.3333333333)">\n'
    + MARK.replace("currentColor", INK) + "\n</g>",
    "0 0 1024 1024",
)
for name, document in (("app-icon", app), ("github-avatar", avatar)):
    (BRAND / f"{name}.svg").write_text(document)
    render(document, 1024).save(BRAND / f"{name}.png")

app_image = render(app, 1024)
app_image.save(BRAND / "app-icon.icns", format="ICNS")
app_image.save(BRAND / "app-icon.ico", format="ICO", sizes=[(n, n) for n in (16, 24, 32, 48, 64, 128, 256)])

favicon = svg(
    f"<style>svg{{color:{INK}}}@media(prefers-color-scheme:dark){{svg{{color:{CYAN}}}}}</style>\n"
    + MARK
)
(BRAND / "favicon.svg").write_text(favicon)
for size in (16, 32):
    render(avatar, size).save(BRAND / f"favicon-{size}.png")
render(avatar, 256).save(BRAND / "favicon.ico", format="ICO", sizes=[(16, 16), (32, 32), (48, 48)])

native_icons = ROOT / "apps" / "desktop" / "src-tauri" / "icons"
native_icons.mkdir(parents=True, exist_ok=True)
for extension in ("png", "icns", "ico"):
    shutil.copyfile(BRAND / f"app-icon.{extension}", native_icons / f"icon.{extension}")
# Tauri's macOS tray implementation displays images at 18 logical points.
render(black, 36).save(native_icons / "tray-on-template.png")
render(black_off, 36).save(native_icons / "tray-off-template.png")
render(tray_on, 32).save(native_icons / "tray-on.png")
render(tray_off, 32).save(native_icons / "tray-off.png")
public = ROOT / "apps" / "desktop" / "public"
public.mkdir(parents=True, exist_ok=True)
for name in ("favicon.svg", "favicon.ico"):
    shutil.copyfile(BRAND / name, public / name)

# A review sheet made from the same source assets, with small icons at actual size.
sheet = Image.new("RGB", (1200, 944), MIST)
draw = ImageDraw.Draw(sheet)
font_path = "/System/Library/Fonts/Helvetica.ttc"
try:
    heading = ImageFont.truetype(font_path, 30)
    label = ImageFont.truetype(font_path, 19)
    wordmark = ImageFont.truetype(font_path, 54)
except OSError:
    heading = ImageFont.load_default(size=30)
    label = ImageFont.load_default(size=19)
    wordmark = ImageFont.load_default(size=54)


def place(document, size, position):
    im = render(document, size)
    sheet.paste(im, position, im)


draw.text((52, 40), "Glassview / Ripple", font=heading, fill=INK)
draw.text((52, 108), "App icon", font=label, fill=INK)
place(app, 248, (48, 150))
draw.text((416, 108), "Website", font=label, fill=INK)
place(dark, 80, (410, 185))
draw.text((506, 191), "Glassview", font=wordmark, fill=INK)
draw.rounded_rectangle((398, 294, 1148, 415), 20, fill=INK)
place(accent, 80, (410, 315))
draw.text((506, 321), "Glassview", font=wordmark, fill="#FFFFFF")
draw.text((52, 471), "GitHub avatar", font=label, fill=INK)
place(avatar, 136, (60, 516))
draw.text((416, 471), "Menu bar / actual pixel sizes", font=label, fill=INK)
draw.rounded_rectangle((398, 518, 1148, 578), 12, fill="#E2E9EB")
draw.rounded_rectangle((398, 592, 1148, 652), 12, fill=INK)
for x, size in ((432, 16), (624, 22), (820, 32)):
    place(dark, size, (x, 548 - size // 2))
    place(white, size, (x, 622 - size // 2))
    draw.text((x + 48, 536), f"{size} px", font=label, fill=INK)
    draw.text((x + 48, 610), f"{size} px", font=label, fill="#FFFFFF")
draw.text((52, 711), "Tray on / off", font=label, fill=INK)
draw.text((416, 711), "macOS template at 18 and 36 px, Windows at 32 px", font=label, fill=INK)
draw.rounded_rectangle((398, 758, 1148, 818), 12, fill="#E2E9EB")
draw.rounded_rectangle((398, 832, 1148, 892), 12, fill=INK)
dark_off = svg(OFF_MARK.replace("currentColor", INK))
white_off = svg(OFF_MARK.replace("currentColor", "#FFFFFF"))
for x, size, on_light, off_light, on_dark, off_dark in (
    (432, 18, dark, dark_off, white, white_off),
    (560, 36, dark, dark_off, white, white_off),
    (760, 32, tray_on, tray_off, tray_on, tray_off),
):
    place(on_light, size, (x, 788 - size // 2))
    place(off_light, size, (x + size + 16, 788 - size // 2))
    place(on_dark, size, (x, 862 - size // 2))
    place(off_dark, size, (x + size + 16, 862 - size // 2))
place(tray_on, 52, (60, 770))
place(tray_off, 52, (128, 770))
sheet.save(BRAND / "preview.png")
print(f"Generated logo assets in {BRAND}")

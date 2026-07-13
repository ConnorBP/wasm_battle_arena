from pathlib import Path
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
source = Image.open(ROOT / "assets/textures/character/ghost_base.png").convert("RGBA")
out = ROOT / "assets/textures/character/cosmetics"
out.mkdir(parents=True, exist_ok=True)

def save(name, pixels):
    image = source.copy()
    for x, y, color in pixels:
        image.putpixel((x, y), color)
    image.save(out / name, optimize=True)

GOLD = (255, 205, 55, 255)
DARK_GOLD = (185, 118, 30, 255)
PURPLE = (115, 64, 190, 255)
BLUE = (60, 120, 225, 255)
PINK = (238, 70, 160, 255)
DARK_PINK = (155, 35, 105, 255)

save("ghost_crown.png", [
    (5, 3, GOLD), (7, 2, GOLD), (9, 3, GOLD),
    (5, 4, DARK_GOLD), (6, 4, GOLD), (7, 4, GOLD), (8, 4, GOLD), (9, 4, DARK_GOLD),
])
save("ghost_wizard.png", [
    (7, 0, PURPLE), (6, 1, PURPLE), (7, 1, BLUE),
    (5, 2, PURPLE), (6, 2, BLUE), (7, 2, BLUE), (8, 2, PURPLE),
    (4, 3, PURPLE), (5, 3, PURPLE), (6, 3, BLUE), (7, 3, BLUE), (8, 3, PURPLE), (9, 3, PURPLE),
    (3, 4, PURPLE), (4, 4, PURPLE), (5, 4, PURPLE), (6, 4, PURPLE), (7, 4, PURPLE), (8, 4, PURPLE), (9, 4, PURPLE), (10, 4, PURPLE),
])
save("ghost_bow.png", [
    (4, 10, DARK_PINK), (5, 10, PINK), (6, 10, DARK_PINK),
    (7, 10, PINK),
    (8, 10, DARK_PINK), (9, 10, PINK), (10, 10, DARK_PINK),
    (5, 11, DARK_PINK), (7, 11, DARK_PINK), (9, 11, DARK_PINK),
])
print(f"generated 3 cosmetics in {out}")

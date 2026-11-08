"""Renders icon.png (python tools/make_icon.py icon.png): a half-built castle in isometric view. The built part
is stone bricks; the rest is the schematic's ghost blocks, with the printer's beam placing the next one, in front
of a horizon sunrise. Drawn supersampled, then scaled down to 256x256."""
import math
import random
import sys

from PIL import Image, ImageDraw, ImageFilter

SS = 4
N = 256
S = N * SS
random.seed(7)


def lerp(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(len(a)))


def shade(c, f):
    return tuple(max(0, min(255, int(ch * f))) for ch in c)


# --- background: night-to-dawn gradient, horizon glow, stars ---
bg = Image.new("RGBA", (S, S))
px = bg.load()
HY = 0.72
for y in range(S):
    t = y / S
    if t < HY:
        c = lerp((14, 18, 46), (60, 34, 98), (t / HY) ** 1.6)
    else:
        c = lerp((255, 150, 80), (120, 40, 70), min(1, (t - HY) / (1 - HY)) ** 0.8)
    for x in range(S):
        px[x, y] = c + (255,)

glow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
gd = ImageDraw.Draw(glow)
cx, cy = S // 2, int(S * HY)
for i in range(40, 0, -1):
    r = S * 0.012 * i
    gd.ellipse((cx - r * 1.6, cy - r, cx + r * 1.6, cy + r), fill=(255, 170, 90, min(255, (10 + 3 * (40 - i)) // 2)))
bg = Image.alpha_composite(bg, glow.filter(ImageFilter.GaussianBlur(S * 0.02)))
d = ImageDraw.Draw(bg)
d.line((0, cy, S, cy), fill=(255, 220, 160, 255), width=int(S * 0.006))
for _ in range(45):
    x, y = random.randrange(S), random.randrange(int(S * 0.45))
    r = random.choice((1, 1, 2)) * SS
    d.ellipse((x - r, y - r, x + r, y + r), fill=(255, 255, 255, random.randrange(90, 220)))

# --- the castle: (x, y, z) -> kind, x and z in 0..W-1, y up ---
W = 7
blocks = {}
for x in range(W):
    for z in range(W):
        blocks[(x, 0, z)] = "grass"


def put(x, y, z, kind):
    blocks[(x, y, z)] = kind


TOWERS = {  # corner -> height of its built part (above it: ghost up to the top)
    (0, 0): 4, (5, 0): 3, (0, 5): 5, (5, 5): 3,
}
TOWER_TOP = 4  # tower body y=1..4, battlements at y=5
for (tx0, tz0), built in TOWERS.items():
    for x in (tx0, tx0 + 1):
        for z in (tz0, tz0 + 1):
            for y in range(1, TOWER_TOP + 1):
                put(x, y, z, "stone" if y <= built else "ghost")
    for (x, z) in ((tx0, tz0), (tx0 + 1, tz0 + 1)):  # two battlement blocks per tower
        put(x, TOWER_TOP + 1, z, "stone" if TOWER_TOP + 1 <= built else "ghost")

WALL_TOP = 2  # wall body y=1..2, battlements at y=3 on every other block


def wall(cells, built):
    for i, (x, z) in enumerate(cells):
        for y in range(1, WALL_TOP + 1):
            put(x, y, z, "stone" if y <= built(i, y) else "ghost")
        if i % 2 == 0:
            put(x, WALL_TOP + 1, z, "stone" if WALL_TOP + 1 <= built(i, WALL_TOP + 1) else "ghost")


mid = range(2, 5)
wall([(0, z) for z in mid], lambda i, y: 3)                    # left wall: finished
wall([(x, 0) for x in mid], lambda i, y: 3 if i == 0 else 2)   # back wall: battlements missing
wall([(6, z) for z in mid], lambda i, y: 2)                    # right wall: battlements missing
wall([(x, 6) for x in mid], lambda i, y: 3 if i == 0 else 2)   # front wall with the gate
put(3, 1, 6, "door")
put(3, 2, 6, "door")


def is_solid(p):
    return blocks.get(p) not in (None, "ghost")


# --- isometric projection, centred on the castle ---
u = S * 0.054
cos30 = math.cos(math.radians(30))


def raw(x, y, z):
    return ((x - z) * u * cos30, (x + z) * u * 0.5 - y * u)


c0 = raw(W / 2, 3.0, W / 2)
ox, oy = S * 0.5 - c0[0], S * 0.53 - c0[1]


def proj(x, y, z):
    rx, ry = raw(x, y, z)
    return (ox + rx, oy + ry)


def cube_faces(x, y, z):
    P = lambda dx, dy, dz: proj(x + dx, y + dy, z + dz)
    top = [P(0, 1, 0), P(1, 1, 0), P(1, 1, 1), P(0, 1, 1)]
    right = [P(1, 1, 0), P(1, 1, 1), P(1, 0, 1), P(1, 0, 0)]  # +x face
    left = [P(0, 1, 1), P(1, 1, 1), P(1, 0, 1), P(0, 0, 1)]   # +z face
    return top, right, left


def texels(face, sub, base, jitter):
    """Splits a parallelogram face into sub x sub texels with random brightness jitter."""
    a, b, _, dd = face

    def pt(fi, fj):
        return (a[0] + (b[0] - a[0]) * fi + (dd[0] - a[0]) * fj, a[1] + (b[1] - a[1]) * fi + (dd[1] - a[1]) * fj)

    for i in range(sub):
        for j in range(sub):
            q = [pt(i / sub, j / sub), pt((i + 1) / sub, j / sub), pt((i + 1) / sub, (j + 1) / sub),
                 pt(i / sub, (j + 1) / sub)]
            yield q, shade(base, 1 + random.uniform(-jitter, jitter))


GRASS = (98, 176, 66)
DIRT = (134, 96, 67)
STONE = (150, 152, 164)
DOOR = (96, 62, 36)
GHOST = (80, 220, 255)

layer = Image.new("RGBA", (S, S), (0, 0, 0, 0))
ld = ImageDraw.Draw(layer, "RGBA")
for p in sorted(blocks, key=lambda p: (p[0] + p[1] + p[2], p[1])):
    kind = blocks[p]
    x, y, z = p
    top, right, left = cube_faces(x, y, z)
    occl = (lambda q: q in blocks) if kind == "ghost" else is_solid
    faces = [(f, k) for f, k, nb in ((left, 0.8, (x, y, z + 1)), (right, 0.6, (x + 1, y, z)), (top, 1.0, (x, y + 1, z)))
             if not occl(nb)]
    if kind == "ghost":
        for f, k in faces:
            ld.polygon(f, fill=shade(GHOST, k) + (50,))
        for f, _ in faces:
            ld.line(f + [f[0]], fill=(170, 245, 255, 255), width=int(SS * 1.6), joint="curve")
        continue
    for f, k in faces:
        if kind == "grass":
            base = GRASS if f is top else DIRT
        else:
            base = {"stone": STONE, "door": DOOR}[kind]
        ld.polygon(f, fill=shade(base, k) + (255,), outline=shade(base, k) + (255,))  # no seams between texels
        for q, col in texels(f, 4 if kind != "door" else 2, shade(base, k), 0.08):
            ld.polygon(q, fill=col + (255,))
        a, b, c, dd = f
        if kind == "grass" and f is not top:  # grass fringe on the side faces
            ld.polygon([a, b, (b[0] + (c[0] - b[0]) * 0.25, b[1] + (c[1] - b[1]) * 0.25),
                        (a[0] + (dd[0] - a[0]) * 0.25, a[1] + (dd[1] - a[1]) * 0.25)], fill=shade(GRASS, k * 0.95) + (255,))
        if kind == "stone" and f is not top:  # brick mortar line across the middle
            ld.line([((a[0] + dd[0]) / 2, (a[1] + dd[1]) / 2), ((b[0] + c[0]) / 2, (b[1] + c[1]) / 2)],
                    fill=shade(STONE, k * 0.7) + (255,), width=SS)
        ld.line(f + [f[0]], fill=shade(base, k * 0.55) + (255,), width=SS)

# a red flag on the finished front-left tower
fx, fy = proj(0.5, TOWER_TOP + 2, 5.5)
pole_top = (fx, fy - u * 1.5)
ld.line((fx, fy, pole_top[0], pole_top[1]), fill=(70, 50, 40, 255), width=int(SS * 2.2))
ld.polygon([pole_top, (pole_top[0] + u * 0.95, pole_top[1] + u * 0.3), (pole_top[0], pole_top[1] + u * 0.6)],
           fill=(220, 50, 50, 255))

# printer beam: from the nozzle into the ghost block being placed (front-right tower, next layer)
tx, ty = proj(6.0, 4.5, 6.0)
nx, ny = tx + S * 0.13, ty - S * 0.15
beam = Image.new("RGBA", (S, S), (0, 0, 0, 0))
bd = ImageDraw.Draw(beam)
bd.polygon([(nx - S * 0.01, ny), (nx + S * 0.01, ny), (tx + S * 0.04, ty), (tx - S * 0.04, ty)], fill=(255, 230, 140, 120))
beam = beam.filter(ImageFilter.GaussianBlur(S * 0.006))
bd = ImageDraw.Draw(beam)
bd.line((nx, ny, tx, ty), fill=(255, 245, 200, 255), width=SS * 2)
r = S * 0.026
bd.ellipse((nx - r, ny - r, nx + r, ny + r), fill=(255, 210, 110, 255), outline=(255, 250, 230, 255), width=SS * 2)
for ang in range(0, 360, 45):
    L = S * (0.045 if ang % 90 == 0 else 0.025)
    bd.line((tx, ty, tx + L * math.cos(math.radians(ang)), ty + L * math.sin(math.radians(ang))),
            fill=(255, 250, 220, 255), width=int(SS * 1.8))

img = Image.alpha_composite(Image.alpha_composite(bg, layer), beam)
mask = Image.new("L", (S, S), 0)
ImageDraw.Draw(mask).rounded_rectangle((0, 0, S - 1, S - 1), radius=int(S * 0.2), fill=255)
img.putalpha(mask)
img.resize((N, N), Image.LANCZOS).save(sys.argv[1], optimize=True)
print("ok")

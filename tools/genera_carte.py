#!/usr/bin/python3
"""Genera un mazzo originale in stile "Piacentine" (40 carte + dorso) in SVG.

Tutta la grafica e' disegnata da questo script a partire da primitive
geometriche (cerchi, ellissi, curve di Bezier): nessuna immagine esistente e'
copiata o ricalcata, quindi l'output e' rilasciabile con la stessa licenza
MIT del progetto.

Uso:  py tools/genera_carte.py   (scrive in crates/bazzica-app/public/carte/)

Lo script usa solo la libreria standard ed e' deterministico.
"""

from __future__ import annotations

import math
import os
import sys

W, H = 240, 400

# Palette (colori piatti, contorni scuri)
DARK = "#2a1d14"
CREAM = "#fbf5e6"
RED = "#c8372d"
BLUE = "#2f5fa8"
YELLOW = "#f0c33c"
GREEN = "#3d8a4a"
GOLD = "#e0a92e"
SKIN = "#f3cfa6"
WHITE = "#fffdf7"
BROWN = "#7a4a26"
STEEL = "#5b8fd0"
STEEL_HI = "#dbe8f6"
WOOD = "#5c8a34"
WOOD_DK = "#3f6424"

SEMI = ["denari", "coppe", "spade", "bastoni"]
FONT = "Georgia,'Times New Roman',serif"


def f(x: float) -> str:
    """Formatta un numero in modo compatto e deterministico."""
    s = f"{x:.1f}"
    if s.endswith(".0"):
        s = s[:-2]
    if s == "-0":
        s = "0"
    return s


def pts(points) -> str:
    return " ".join(f"{f(x)},{f(y)}" for x, y in points)


# --------------------------------------------------------------------------
# Geometria
# --------------------------------------------------------------------------

def bez(p0, p1, p2, p3, t):
    u = 1 - t
    x = u**3 * p0[0] + 3 * u * u * t * p1[0] + 3 * u * t * t * p2[0] + t**3 * p3[0]
    y = u**3 * p0[1] + 3 * u * u * t * p1[1] + 3 * u * t * t * p2[1] + t**3 * p3[1]
    return x, y


def bez_d(p0, p1, p2, p3, t):
    u = 1 - t
    dx = 3 * u * u * (p1[0] - p0[0]) + 6 * u * t * (p2[0] - p1[0]) + 3 * t * t * (p3[0] - p2[0])
    dy = 3 * u * u * (p1[1] - p0[1]) + 6 * u * t * (p2[1] - p1[1]) + 3 * t * t * (p3[1] - p2[1])
    n = math.hypot(dx, dy) or 1.0
    return dx / n, dy / n


def ribbon(curve, widthfn, n=14):
    """Poligono che segue una curva di Bezier con larghezza variabile."""
    left, right, centre = [], [], []
    for i in range(n + 1):
        t = i / n
        x, y = bez(*curve, t)
        dx, dy = bez_d(*curve, t)
        nx, ny = -dy, dx
        w = widthfn(t) / 2
        left.append((x + nx * w, y + ny * w))
        right.append((x - nx * w, y - ny * w))
        centre.append((x, y))
    poly = left + right[::-1]
    return poly, centre


# --------------------------------------------------------------------------
# Semi (pip) disegnati centrati sull'origine
# --------------------------------------------------------------------------

def pip_denaro() -> str:
    """Moneta: anelli concentrici + rosetta, raggio 20."""
    petals = "".join(
        f'<ellipse rx="2.6" ry="6.2" cy="-6" transform="rotate({a})" fill="{RED}"/>'
        for a in range(0, 360, 45)
    )
    dots = "".join(
        f'<circle r="1.1" cx="{f(17.2 * math.cos(math.radians(a)))}" '
        f'cy="{f(17.2 * math.sin(math.radians(a)))}" fill="{DARK}"/>'
        for a in range(0, 360, 30)
    )
    return (
        f'<circle r="20" fill="{GOLD}" stroke="{DARK}" stroke-width="1.6"/>'
        f"{dots}"
        f'<circle r="15" fill="{GREEN}" stroke="{DARK}" stroke-width="1"/>'
        f'<circle r="12.6" fill="{YELLOW}"/>'
        f"{petals}"
        f'<circle r="3.4" fill="{GOLD}" stroke="{DARK}" stroke-width=".8"/>'
    )


def pip_coppa() -> str:
    """Calice: coppa rossa, bordo e stelo dorati, alto circa 48."""
    return (
        f'<path d="M-12,24 Q0,15 12,24 Z" fill="{RED}" stroke="{DARK}" stroke-width="1.4" stroke-linejoin="round"/>'
        f'<path d="M-3,6 L3,6 L4,18 L-4,18 Z" fill="{GOLD}" stroke="{DARK}" stroke-width="1.2"/>'
        f'<ellipse cy="12" rx="5.5" ry="3" fill="{GOLD}" stroke="{DARK}" stroke-width="1.2"/>'
        f'<path d="M-18,-20 C-18,-2 -9,6 0,6 C9,6 18,-2 18,-20 Z" fill="{RED}" stroke="{DARK}" stroke-width="1.6" stroke-linejoin="round"/>'
        f'<path d="M-15,-9 Q0,-4 15,-9" fill="none" stroke="{GOLD}" stroke-width="2.4"/>'
        f'<path d="M-8,-3 Q0,0 8,-3" fill="none" stroke="{YELLOW}" stroke-width="1.2"/>'
        f'<ellipse cy="-20" rx="18" ry="4.5" fill="{GOLD}" stroke="{DARK}" stroke-width="1.4"/>'
        f'<ellipse cy="-20.5" rx="13" ry="2.4" fill="#8a2219"/>'
    )


def sword(p0, c1, c2, p3, width=8.0, guard=24.0) -> str:
    """Spada ricurva (scimitarra): elsa in p0, punta in p3."""
    def wfn(t):
        if t < 0.82:
            return width * (0.85 + 0.35 * t)
        return width * 1.14 * max(0.0, (1 - t) / 0.18) ** 0.8

    poly, centre = ribbon((p0, c1, c2, p3), wfn, n=14)
    dx, dy = bez_d(p0, c1, c2, p3, 0.0)
    nx, ny = -dy, dx
    g = guard / 2
    ga = (p0[0] + nx * g + dx * 3, p0[1] + ny * g + dy * 3)
    gb = (p0[0] - nx * g + dx * 3, p0[1] - ny * g + dy * 3)
    gm = (p0[0] - dx * 2, p0[1] - dy * 2)
    grip_end = (p0[0] - dx * guard * 0.62, p0[1] - dy * guard * 0.62)
    pom = (p0[0] - dx * guard * 0.8, p0[1] - dy * guard * 0.8)
    sw = max(1.0, width / 7)
    fuller = centre[1:11]
    return (
        f'<path d="M{pts(poly)}Z" fill="{STEEL}" stroke="{DARK}" stroke-width="{f(1.3 * sw)}" stroke-linejoin="round"/>'
        f'<polyline points="{pts(fuller)}" fill="none" stroke="{STEEL_HI}" stroke-width="{f(1.4 * sw)}" stroke-linecap="round"/>'
        f'<path d="M{pts([p0, grip_end])}" stroke="{DARK}" stroke-width="{f(7.5 * sw)}" stroke-linecap="round"/>'
        f'<path d="M{pts([p0, grip_end])}" stroke="{RED}" stroke-width="{f(4.6 * sw)}" stroke-linecap="round"/>'
        f'<path d="M{pts([ga, gm, gb])}" fill="none" stroke="{DARK}" stroke-width="{f(7 * sw)}" stroke-linecap="round" stroke-linejoin="round"/>'
        f'<path d="M{pts([ga, gm, gb])}" fill="none" stroke="{GOLD}" stroke-width="{f(4 * sw)}" stroke-linecap="round" stroke-linejoin="round"/>'
        f'<circle cx="{f(pom[0])}" cy="{f(pom[1])}" r="{f(4.4 * sw)}" fill="{GOLD}" stroke="{DARK}" stroke-width="{f(1.2 * sw)}"/>'
    )


def club(a, b, w0=7.0, w1=13.0, bend=0.0) -> str:
    """Bastone nodoso: impugnatura in a, testa in b."""
    ax, ay = a
    bx, by = b
    dx, dy = bx - ax, by - ay
    ln = math.hypot(dx, dy)
    ux, uy = dx / ln, dy / ln
    nx, ny = -uy, ux
    c1 = (ax + dx / 3 + nx * bend, ay + dy / 3 + ny * bend)
    c2 = (ax + 2 * dx / 3 + nx * bend, ay + 2 * dy / 3 + ny * bend)
    curve = (a, c1, c2, b)

    def wfn(t):
        return w0 + (w1 - w0) * t ** 1.4

    poly, centre = ribbon(curve, wfn, n=12)
    sc = w1 / 13
    out = []
    # nodi (monconi dei rami) disegnati sotto il fusto
    knots = []
    for i, t in enumerate((0.3, 0.45, 0.6, 0.75)):
        x, y = bez(*curve, t)
        tx, ty = bez_d(*curve, t)
        side = 1 if i % 2 == 0 else -1
        px, py = -ty * side, tx * side
        w = wfn(t) / 2
        s = (x + px * (w - 1), y + py * (w - 1))
        e = (x + px * (w + 4.5 * sc) + tx * 3 * sc, y + py * (w + 4.5 * sc) + ty * 3 * sc)
        knots.append((s, e))
    for s, e in knots:
        out.append(f'<path d="M{pts([s, e])}" stroke="{DARK}" stroke-width="{f(6.4 * sc)}" stroke-linecap="round"/>')
    for s, e in knots:
        out.append(f'<path d="M{pts([s, e])}" stroke="{WOOD}" stroke-width="{f(3.8 * sc)}" stroke-linecap="round"/>')
    out.append(f'<path d="M{pts(poly)}Z" fill="{WOOD}" stroke="{DARK}" stroke-width="{f(1.3 * max(1, sc))}" stroke-linejoin="round"/>')
    # venatura
    out.append(f'<polyline points="{pts(centre[2:11])}" fill="none" stroke="{WOOD_DK}" stroke-width="{f(1.5 * sc)}" stroke-linecap="round"/>')
    # fasce rosse e puntali dorati
    for t, col, ww in ((0.1, RED, 3.4), (0.9, RED, 3.4)):
        x, y = bez(*curve, t)
        tx, ty = bez_d(*curve, t)
        w = wfn(t) / 2 + 0.6
        s = (x - ty * w, y + tx * w)
        e = (x + ty * w, y - tx * w)
        out.append(f'<path d="M{pts([s, e])}" stroke="{DARK}" stroke-width="{f((ww + 2) * sc)}"/>')
        out.append(f'<path d="M{pts([s, e])}" stroke="{col}" stroke-width="{f(ww * sc)}"/>')
    out.append(f'<circle cx="{f(bx)}" cy="{f(by)}" r="{f(w1 / 2 * 0.75)}" fill="{GOLD}" stroke="{DARK}" stroke-width="{f(1.1 * sc)}"/>')
    out.append(f'<circle cx="{f(ax)}" cy="{f(ay)}" r="{f(w0 / 2 * 0.9)}" fill="{GOLD}" stroke="{DARK}" stroke-width="{f(1.1 * sc)}"/>')
    return "".join(out)


def pip_spada_small() -> str:
    """Piccola spada dritta per gli indici d'angolo (alta ~48)."""
    return sword((0, 16), (3, 4), (3, -10), (-1, -27), width=8, guard=20)


def pip_bastone_small() -> str:
    return club((-5, 22), (5, -24), w0=8, w1=14)


SMALL_PIPS = {
    "denari": pip_denaro,
    "coppe": pip_coppa,
    "spade": pip_spada_small,
    "bastoni": pip_bastone_small,
}


def use(id_, x, y, s=1.0, rot=0.0) -> str:
    tr = f"translate({f(x)},{f(y)})"
    if rot:
        tr += f" rotate({f(rot)})"
    if s != 1.0:
        tr += f" scale({s:g})"
    return f'<use href="#{id_}" transform="{tr}"/>'


# --------------------------------------------------------------------------
# Disposizioni delle carte numerali
# --------------------------------------------------------------------------

GRID = {
    2: [(120, 118), (120, 282)],
    3: [(120, 96), (120, 200), (120, 304)],
    4: [(78, 120), (162, 120), (78, 280), (162, 280)],
    5: [(78, 106), (162, 106), (120, 200), (78, 294), (162, 294)],
    6: [(78, 100), (162, 100), (78, 200), (162, 200), (78, 300), (162, 300)],
    7: [(78, 88), (162, 88), (120, 158), (78, 232), (162, 232), (78, 310), (162, 310)],
}
GRID_SCALE = {2: 2.0, 3: 1.7, 4: 1.75, 5: 1.6, 6: 1.5, 7: 1.36}


def ornament_small(x, y) -> str:
    """Piccolo fregio a quadrifoglio fra i semi."""
    return (
        f'<g transform="translate({f(x)},{f(y)})">'
        f'<path d="M0,-7 Q3,0 0,7 Q-3,0 0,-7Z M-7,0 Q0,3 7,0 Q0,-3 -7,0Z" fill="{GREEN}" stroke="{DARK}" stroke-width=".7"/>'
        f'<circle r="2" fill="{RED}"/></g>'
    )


def numerale_grid(seme, n) -> str:
    out = []
    pid = "p"
    s = GRID_SCALE[n]
    if n in (2, 4, 6):
        out.append(ornament_small(120, 200 if n != 6 else 150))
        if n == 6:
            out.append(ornament_small(120, 250))
    for x, y in GRID[n]:
        out.append(use(pid, x, y, s))
    return "".join(out)


def numerale_spade(n) -> str:
    pairs = n // 2
    out = []
    # coppie di spade ricurve che si incrociano in alto e in basso
    spreads = {1: [96], 2: [100, 50], 3: [104, 70, 36]}[pairs]
    for k, bow in enumerate(spreads):
        flip = k % 2 == 1
        y0, y1 = (338, 62) if not flip else (62, 338)
        cross = 22
        dy = -78 if not flip else 78
        for side in (-1, 1):
            hx = 120 - side * cross
            p0 = (hx, y0)
            p3 = (hx, y1)
            c1 = (120 + side * bow * 1.25, y0 + dy)
            c2 = (120 + side * bow * 1.25, y1 - dy)
            out.append(sword(p0, c1, c2, p3, width=11, guard=30))
    if n % 2:
        out.append(sword((120, 318), (128, 240), (112, 150), (120, 78), width=12, guard=32))
    return "".join(out)


def numerale_bastoni(n) -> str:
    pairs = n // 2
    out = []
    offs = {1: [0], 2: [-30, 30], 3: [-50, 0, 50]}[pairs]
    span = {1: 58, 2: 46, 3: 36}[pairs]
    for side in (-1, 1):
        for o in offs:
            a = (120 + o - side * span, 334)
            b = (120 + o + side * span, 66)
            out.append(club(a, b, w0=9, w1=17, bend=5 * side))
    if n % 2:
        out.append(club((120, 340), (120, 60), w0=10, w1=18))
    return "".join(out)


# --------------------------------------------------------------------------
# Assi
# --------------------------------------------------------------------------

def asso(seme) -> str:
    if seme == "denari":
        rays = "".join(
            f'<path d="M0,-74 L7,-60 L-7,-60Z" transform="rotate({a})" fill="{RED}" stroke="{DARK}" stroke-width="1"/>'
            for a in range(0, 360, 30)
        )
        leaves = "".join(
            f'<ellipse cy="-66" rx="4.5" ry="9" transform="rotate({a})" fill="{GREEN}" stroke="{DARK}" stroke-width=".9"/>'
            for a in range(15, 360, 30)
        )
        return (
            f'<g transform="translate(120,200)">'
            f'<circle r="62" fill="{CREAM}" stroke="{DARK}" stroke-width="1"/>'
            f"{leaves}{rays}"
            f'<circle r="58" fill="{RED}" stroke="{DARK}" stroke-width="1.6"/>'
            f'<circle r="54" fill="none" stroke="{YELLOW}" stroke-width="1.6" stroke-dasharray="3 3"/>'
            f'<g transform="scale(2.5)">{pip_denaro()}</g>'
            f"</g>"
            + "".join(ornament_small(x, y) for x, y in ((120, 82), (120, 318)))
        )
    if seme == "coppe":
        return (
            f'<g transform="translate(120,200)">'
            f'<path d="M-46,-78 Q-70,-40 -58,-8 M46,-78 Q70,-40 58,-8" fill="none" stroke="{GREEN}" stroke-width="3"/>'
            + "".join(
                f'<ellipse cx="{f(sx * 62)}" cy="{f(y)}" rx="4.5" ry="8" transform="rotate({f(sx * 20)} {f(sx * 62)} {f(y)})" fill="{GREEN}" stroke="{DARK}" stroke-width=".8"/>'
                for sx in (-1, 1) for y in (-62, -40, -20)
            )
            + f'<g transform="scale(3.6)">{pip_coppa()}</g>'
            f'<g transform="translate(0,-34)">{pip_denaro_mini()}</g>'
            f'<path d="M-30,-86 Q-15,-104 0,-96 Q15,-104 30,-86" fill="none" stroke="{RED}" stroke-width="3" stroke-linecap="round"/>'
            f'<circle cy="-100" r="5" fill="{GOLD}" stroke="{DARK}" stroke-width="1.2"/>'
            f"</g>"
        )
    if seme == "spade":
        leaves = "".join(
            f'<ellipse cx="{f(120 + sx * (14 + i * 9))}" cy="{f(318 - i * 13)}" rx="4" ry="9" '
            f'transform="rotate({f(sx * (40 + i * 8))} {f(120 + sx * (14 + i * 9))} {f(318 - i * 13)})" '
            f'fill="{GREEN}" stroke="{DARK}" stroke-width=".8"/>'
            for sx in (-1, 1) for i in range(5)
        )
        return (
            leaves
            + f'<path d="M58,262 Q120,300 182,262" fill="none" stroke="{RED}" stroke-width="5" stroke-linecap="round"/>'
            + sword((120, 312), (150, 230), (150, 120), (110, 48), width=20, guard=64)
            + f'<path d="M78,150 Q120,128 162,150 L156,164 Q120,144 84,164Z" fill="{RED}" stroke="{DARK}" stroke-width="1.2"/>'
            + f'<path d="M84,164 L72,176 L78,150Z M156,164 L168,176 L162,150Z" fill="#8a2219" stroke="{DARK}" stroke-width="1"/>'
        )
    # bastoni
    leaves = "".join(
        f'<ellipse cx="{f(x)}" cy="{f(y)}" rx="5" ry="11" transform="rotate({f(r)} {f(x)} {f(y)})" fill="{GREEN}" stroke="{DARK}" stroke-width=".9"/>'
        for x, y, r in ((98, 118, -50), (146, 156, 50), (92, 214, -55), (150, 250, 55), (104, 82, -30), (140, 96, 35))
    )
    return (
        leaves
        + club((120, 342), (120, 62), w0=20, w1=40, bend=10)
        + f'<path d="M78,290 Q120,270 162,290" fill="none" stroke="{RED}" stroke-width="5" stroke-linecap="round"/>'
    )


def pip_denaro_mini() -> str:
    return f'<g transform="scale(.42)">{pip_denaro()}</g>'


# --------------------------------------------------------------------------
# Figure (a due teste)
# --------------------------------------------------------------------------

PAL = {
    # main, secondary, accent
    "denari": (RED, GREEN, YELLOW),
    "coppe": (BLUE, YELLOW, RED),
    "spade": (YELLOW, RED, BLUE),
    "bastoni": (GREEN, RED, BLUE),
}

OUT = f'stroke="{DARK}" stroke-width="1.6" stroke-linejoin="round"'


def face(cx, cy, rx=18, ry=21, beard=None, hair=BROWN) -> str:
    s = []
    s.append(f'<ellipse cx="{f(cx)}" cy="{f(cy)}" rx="{f(rx)}" ry="{f(ry)}" fill="{SKIN}" {OUT}/>')
    ey = cy - 1
    s.append(f'<circle cx="{f(cx - 6.5)}" cy="{f(ey)}" r="1.9" fill="{DARK}"/>')
    s.append(f'<circle cx="{f(cx + 6.5)}" cy="{f(ey)}" r="1.9" fill="{DARK}"/>')
    s.append(f'<path d="M{f(cx - 10)},{f(ey - 5)} Q{f(cx - 6)},{f(ey - 8)} {f(cx - 3)},{f(ey - 5.5)} M{f(cx + 3)},{f(ey - 5.5)} Q{f(cx + 6)},{f(ey - 8)} {f(cx + 10)},{f(ey - 5)}" fill="none" stroke="{DARK}" stroke-width="1.2" stroke-linecap="round"/>')
    s.append(f'<path d="M{f(cx)},{f(ey + 1)} L{f(cx - 3)},{f(ey + 9)} L{f(cx + 1.5)},{f(ey + 9)}" fill="none" stroke="{DARK}" stroke-width="1.1" stroke-linejoin="round"/>')
    s.append(f'<circle cx="{f(cx - 10)}" cy="{f(ey + 8)}" r="3" fill="#e9a08a"/><circle cx="{f(cx + 10)}" cy="{f(ey + 8)}" r="3" fill="#e9a08a"/>')
    if beard:
        s.append(
            f'<path d="M{f(cx - rx + 1)},{f(cy + 2)} Q{f(cx - rx + 2)},{f(cy + ry + 10)} {f(cx)},{f(cy + ry + 16)} '
            f'Q{f(cx + rx - 2)},{f(cy + ry + 10)} {f(cx + rx - 1)},{f(cy + 2)} Q{f(cx + 8)},{f(cy + 15)} {f(cx)},{f(cy + 15)} '
            f'Q{f(cx - 8)},{f(cy + 15)} {f(cx - rx + 1)},{f(cy + 2)}Z" fill="{beard}" {OUT}/>'
        )
        s.append(f'<path d="M{f(cx - 8)},{f(cy + 13)} Q{f(cx)},{f(cy + 8)} {f(cx + 8)},{f(cy + 13)}" fill="none" stroke="{DARK}" stroke-width="2.2" stroke-linecap="round"/>')
    else:
        s.append(f'<path d="M{f(cx - 5)},{f(cy + 12)} Q{f(cx)},{f(cy + 15)} {f(cx + 5)},{f(cy + 12)}" fill="none" stroke="#a3392f" stroke-width="1.6" stroke-linecap="round"/>')
    return "".join(s)


def held(seme, x, y, s=1.0, rider=False) -> str:
    """Simbolo del seme tenuto in mano, centro in (x, y)."""
    if seme == "denari":
        return use("p", x, y, 1.15 * s)
    if seme == "coppe":
        return use("p", x, y, 1.05 * s)
    if seme == "spade":
        return sword((x, y + 30 * s), (x + 4, y + 2), (x + 10 * s, y - 30 * s), (x + 2, y - 62 * s), width=7.5, guard=24)
    return club((x - 3, y + 34 * s), (x + 6, y - 58 * s), w0=7, w1=13, bend=4)


def hand(x, y) -> str:
    return f'<circle cx="{f(x)}" cy="{f(y)}" r="6.5" fill="{SKIN}" {OUT}/>'


def fante(seme) -> str:
    m, sc, ac = PAL[seme]
    hx, hy = 176, 116  # mano che regge il seme
    obj_y = {"denari": 90, "coppe": 92, "spade": 92, "bastoni": 92}[seme]
    obj_x = 178
    s = []
    # capelli dietro
    s.append(f'<path d="M96,70 Q94,104 102,112 L138,112 Q146,104 144,70 Q120,40 96,70Z" fill="{BROWN}" {OUT}/>')
    # busto
    s.append(f'<path d="M46,206 L48,156 Q54,120 96,112 L144,112 Q186,120 192,156 L194,206Z" fill="{m}" {OUT}/>')
    s.append(f'<path d="M104,112 L136,112 L131,206 L109,206Z" fill="{sc}" {OUT}/>')
    for yy in (130, 150, 170, 190):
        s.append(f'<circle cx="120" cy="{yy}" r="2.6" fill="{GOLD}" stroke="{DARK}" stroke-width=".8"/>')
    # cintura
    s.append(f'<path d="M48,184 L192,184 L192,194 L48,194Z" fill="{BROWN}" {OUT}/>')
    s.append(f'<rect x="113" y="182" width="14" height="14" rx="2" fill="{GOLD}" {OUT}/>')
    # braccio sinistro (dello spettatore) sul fianco
    s.append(f'<path d="M62,140 Q54,166 68,186" fill="none" stroke="{DARK}" stroke-width="17" stroke-linecap="round"/>')
    s.append(f'<path d="M62,140 Q54,166 68,186" fill="none" stroke="{m}" stroke-width="13" stroke-linecap="round"/>')
    s.append(hand(70, 186))
    # collo e colletto
    s.append(f'<path d="M112,96 L128,96 L129,114 L111,114Z" fill="{SKIN}" {OUT}/>')
    s.append(f'<path d="M96,110 Q120,132 144,110 L140,106 Q120,122 100,106Z" fill="{WHITE}" {OUT}/>')
    s.append(face(120, 78))
    # berretto con piuma
    s.append(f'<path d="M150,52 Q176,30 186,44 Q170,40 156,56Z" fill="{WHITE}" {OUT}/>')
    s.append(f'<path d="M92,62 Q90,38 120,36 Q152,38 150,62 Q120,54 92,62Z" fill="{ac}" {OUT}/>')
    s.append(f'<path d="M88,64 Q120,52 152,64 L152,58 Q120,46 88,58Z" fill="{sc}" {OUT}/>')
    # braccio destro alzato
    s.append(f'<path d="M168,140 Q182,132 {hx},{hy}" fill="none" stroke="{DARK}" stroke-width="17" stroke-linecap="round"/>')
    s.append(f'<path d="M168,140 Q182,132 {hx},{hy}" fill="none" stroke="{m}" stroke-width="13" stroke-linecap="round"/>')
    s.append(held(seme, obj_x, obj_y))
    s.append(hand(hx, hy))
    return "".join(s)


def re_(seme) -> str:
    m, sc, ac = PAL[seme]
    s = []
    # mantello
    s.append(f'<path d="M38,206 L42,152 Q50,114 96,106 L144,106 Q190,114 198,152 L202,206Z" fill="{m}" {OUT}/>')
    s.append(f'<path d="M106,120 L134,120 L138,206 L102,206Z" fill="{sc}" {OUT}/>')
    s.append(f'<path d="M112,140 L128,140 M110,160 L130,160 M108,180 L132,180" stroke="{GOLD}" stroke-width="3"/>')
    # collo di ermellino
    s.append(f'<path d="M58,136 Q90,108 120,124 Q150,108 182,136 Q170,150 150,146 Q134,140 120,150 Q106,140 90,146 Q70,150 58,136Z" fill="{WHITE}" {OUT}/>')
    for x, y in ((76, 134), (98, 128), (142, 128), (164, 134), (120, 138)):
        s.append(f'<path d="M{x},{y - 3} L{x + 2},{y + 3} L{x - 2},{y + 3}Z" fill="{DARK}"/>')
    # capelli
    s.append(f'<path d="M98,72 Q92,106 104,112 L136,112 Q148,106 142,72Z" fill="#8a8a8a" {OUT}/>')
    s.append(face(120, 80, beard="#9a9a9a"))
    # corona
    s.append(f'<path d="M94,62 L92,34 L104,48 L112,28 L120,44 L128,28 L136,48 L148,34 L146,62Z" fill="{GOLD}" {OUT}/>')
    s.append(f'<rect x="92" y="56" width="56" height="9" rx="2" fill="{YELLOW}" {OUT}/>')
    for x in (102, 120, 138):
        s.append(f'<circle cx="{x}" cy="60.5" r="2.6" fill="{RED if x != 120 else BLUE}" stroke="{DARK}" stroke-width=".7"/>')
    for x, y in ((92, 34), (112, 28), (128, 28), (148, 34)):
        s.append(f'<circle cx="{x}" cy="{y}" r="2.6" fill="{WHITE}" stroke="{DARK}" stroke-width=".8"/>')
    # braccio con il seme
    s.append(f'<path d="M172,154 Q186,140 178,124" fill="none" stroke="{DARK}" stroke-width="18" stroke-linecap="round"/>')
    s.append(f'<path d="M172,154 Q186,140 178,124" fill="none" stroke="{sc}" stroke-width="14" stroke-linecap="round"/>')
    s.append(held(seme, 180, 96))
    s.append(hand(178, 122))
    # scettro nell'altra mano
    s.append(f'<path d="M58,196 L72,112" stroke="{DARK}" stroke-width="6" stroke-linecap="round"/>')
    s.append(f'<path d="M58,196 L72,112" stroke="{GOLD}" stroke-width="3.4" stroke-linecap="round"/>')
    s.append(f'<circle cx="73" cy="106" r="6" fill="{GOLD}" {OUT}/><circle cx="73" cy="106" r="2.4" fill="{RED}"/>')
    s.append(hand(66, 168))
    return "".join(s)


def cavallo(seme) -> str:
    m, sc, ac = PAL[seme]
    horse = "#f1ebe0" if seme in ("coppe", "bastoni") else "#b9814c"
    mane = DARK if horse != "#f1ebe0" else "#8a5a32"
    s = []
    # cavaliere (dietro il collo del cavallo)
    s.append(f'<path d="M126,206 L130,104 Q140,86 160,84 Q186,88 194,110 L200,206Z" fill="{m}" {OUT}/>')
    s.append(f'<path d="M150,86 L170,86 L166,140 L154,140Z" fill="{sc}" {OUT}/>')
    s.append(f'<path d="M150,84 L170,84 L168,74 L152,74Z" fill="{SKIN}" {OUT}/>')
    s.append(face(160, 58, rx=15, ry=17))
    # cappello a falda
    s.append(f'<ellipse cx="160" cy="42" rx="26" ry="6" fill="{sc}" {OUT}/>')
    s.append(f'<path d="M144,42 Q146,20 160,20 Q174,20 176,42Z" fill="{ac}" {OUT}/>')
    s.append(f'<path d="M172,26 Q192,10 200,22 Q188,20 176,32Z" fill="{WHITE}" {OUT}/>')
    # cavallo: collo e testa rivolta a sinistra
    s.append(
        f'<path d="M206,206 C200,170 172,128 128,104 L122,98 L120,78 L110,94 '
        f'C88,100 60,118 40,144 C34,154 38,166 50,168 C64,170 78,166 90,158 '
        f'C100,176 104,192 104,206Z" fill="{horse}" {OUT}/>'
    )
    # criniera
    s.append(
        f'<path d="M124,100 C160,120 190,160 200,206 L186,206 C182,176 168,150 150,132 '
        f'L156,126 L142,124 L146,118 L134,114Z" fill="{mane}" {OUT}/>'
    )
    s.append(f'<path d="M112,94 Q100,86 98,98 Q104,96 108,100Z" fill="{mane}" {OUT}/>')
    # occhio, narice, bocca
    s.append(f'<ellipse cx="84" cy="118" rx="4" ry="3" fill="{WHITE}" {OUT}/><circle cx="83" cy="118" r="1.8" fill="{DARK}"/>')
    s.append(f'<ellipse cx="46" cy="152" rx="2.6" ry="1.8" fill="{DARK}"/>')
    s.append(f'<path d="M42,163 Q56,162 66,158" fill="none" stroke="{DARK}" stroke-width="1.2"/>')
    # briglie
    s.append(f'<path d="M56,134 Q62,152 64,166 M58,140 Q90,128 114,100 M66,158 Q100,150 136,152" fill="none" stroke="{RED}" stroke-width="3.2" stroke-linecap="round"/>')
    s.append(f'<circle cx="62" cy="146" r="3" fill="{GOLD}" stroke="{DARK}" stroke-width=".8"/>')
    # bardatura sul collo
    s.append(f'<path d="M112,206 Q140,176 176,184 L180,198 Q146,192 124,206Z" fill="{ac}" {OUT}/>')
    for x, y in ((128, 196), (146, 188), (164, 188)):
        s.append(f'<circle cx="{x}" cy="{y}" r="2.4" fill="{GOLD}" stroke="{DARK}" stroke-width=".7"/>')
    # mano del cavaliere con il seme
    s.append(held(seme, 200, 98, s=0.85))
    s.append(f'<path d="M184,130 Q198,124 198,124" fill="none" stroke="{DARK}" stroke-width="15" stroke-linecap="round"/>')
    s.append(f'<path d="M184,130 Q198,124 198,124" fill="none" stroke="{m}" stroke-width="11" stroke-linecap="round"/>')
    s.append(hand(199, 124))
    return "".join(s)


FIGURE = {8: fante, 9: cavallo, 10: re_}


def figura(seme, n) -> str:
    half = FIGURE[n](seme)
    return (
        f'<clipPath id="c"><rect x="13" y="13" width="214" height="188"/></clipPath>'
        f'<g id="h" clip-path="url(#c)">{half}</g>'
        f'<use href="#h" transform="rotate(180 120 200)"/>'
        f'<path d="M13,200 L227,200" stroke="{DARK}" stroke-width="2"/>'
    )


# --------------------------------------------------------------------------
# Assemblaggio carta
# --------------------------------------------------------------------------

def frame(inner=True) -> str:
    s = (
        f'<rect x="1" y="1" width="238" height="398" rx="14" fill="{CREAM}" stroke="{DARK}" stroke-width="2"/>'
    )
    if inner:
        s += f'<rect x="12" y="12" width="216" height="376" rx="6" fill="none" stroke="{DARK}" stroke-width=".8" opacity=".45"/>'
    return s


def index(seme, n) -> str:
    col = RED if seme in ("denari", "coppe") else DARK
    bg = f'<rect x="4" y="6" width="40" height="84" rx="6" fill="{CREAM}"/>' if n >= 8 else ""
    ls = ' letter-spacing="-3"' if n == 10 else ""
    one = (
        f'<g id="i">{bg}'
        f'<text x="{23 if n < 10 else 22}" y="40" text-anchor="middle" font-family="{FONT}" font-weight="bold" '
        f'font-size="{34 if n < 10 else 29}"{ls} fill="{col}">{n}</text>'
        f'<g transform="translate(23,66) scale(.5)"><use href="#q"/></g></g>'
    )
    return one + '<use href="#i" transform="rotate(180 120 200)"/>'


def svg_doc(body: str, defs: str = "") -> str:
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}">'
        f"<defs>{defs}</defs>{body}</svg>\n"
    )


def carta(seme, n) -> str:
    big_pip = {"denari": pip_denaro, "coppe": pip_coppa}.get(seme)
    if big_pip:
        defs = f'<g id="p">{big_pip()}</g><use id="q" href="#p"/>'
    else:
        defs = f'<g id="q">{SMALL_PIPS[seme]()}</g>'
    if n == 1:
        body = asso(seme)
    elif n <= 7:
        if seme in ("denari", "coppe"):
            body = numerale_grid(seme, n)
        elif seme == "spade":
            body = numerale_spade(n)
        else:
            body = numerale_bastoni(n)
    else:
        body = figura(seme, n)
    return svg_doc(frame(inner=n < 8) + body + index(seme, n), defs)


def dorso() -> str:
    defs = (
        f'<pattern id="l" width="24" height="24" patternUnits="userSpaceOnUse" x="0" y="0">'
        f'<rect width="24" height="24" fill="{BLUE}"/>'
        f'<path d="M12,0 L24,12 L12,24 L0,12Z" fill="{RED}" stroke="{YELLOW}" stroke-width="1.2"/>'
        f'<path d="M12,7 L17,12 L12,17 L7,12Z" fill="{CREAM}"/>'
        f'<circle cx="12" cy="12" r="1.8" fill="{BLUE}"/>'
        f"</pattern>"
    )
    rose = []
    for a in range(0, 360, 30):
        rose.append(f'<ellipse cy="-30" rx="9" ry="24" transform="rotate({a})" fill="{RED}" stroke="{DARK}" stroke-width="1.2"/>')
    for a in range(15, 360, 30):
        rose.append(f'<ellipse cy="-22" rx="5" ry="15" transform="rotate({a})" fill="{YELLOW}" stroke="{DARK}" stroke-width="1"/>')
    body = (
        frame(inner=False)
        + f'<rect x="12" y="12" width="216" height="376" rx="6" fill="url(#l)" stroke="{DARK}" stroke-width="1.5"/>'
        + f'<rect x="18" y="18" width="204" height="364" rx="4" fill="none" stroke="{CREAM}" stroke-width="3"/>'
        + f'<g transform="translate(120,200)">'
        + f'<circle r="66" fill="{CREAM}" stroke="{DARK}" stroke-width="1.6"/>'
        + f'<circle r="60" fill="{GREEN}" stroke="{DARK}" stroke-width="1"/>'
        + f'<circle r="56" fill="none" stroke="{YELLOW}" stroke-width="1.6" stroke-dasharray="4 3"/>'
        + "".join(rose)
        + f'<circle r="14" fill="{GOLD}" stroke="{DARK}" stroke-width="1.4"/>'
        + f'<circle r="7" fill="{BLUE}" stroke="{DARK}" stroke-width="1"/>'
        + f'<circle r="2.5" fill="{CREAM}"/>'
        + "</g>"
        + "".join(
            f'<g transform="translate({x},{y})"><circle r="11" fill="{CREAM}" stroke="{DARK}" stroke-width="1.2"/>'
            f'<circle r="6" fill="{RED}" stroke="{DARK}" stroke-width=".8"/><circle r="2" fill="{YELLOW}"/></g>'
            for x, y in ((40, 40), (200, 40), (40, 360), (200, 360))
        )
    )
    return svg_doc(body, defs)


def anteprima() -> str:
    cells = []
    for seme in SEMI:
        for n in range(1, 11):
            code = f"{seme}_{n:02d}"
            cells.append(f'<figure><img src="{code}.svg" alt="{code}"><figcaption>{code}</figcaption></figure>')
    cells.append('<figure><img src="dorso.svg" alt="dorso"><figcaption>dorso</figcaption></figure>')
    small = "".join(
        f'<img class="s" src="{seme}_{n:02d}.svg" alt="">' for seme in SEMI for n in range(1, 11)
    )
    return (
        "<!doctype html><html lang=\"it\"><head><meta charset=\"utf-8\"><title>Carte Piacentine - anteprima</title>"
        "<style>body{margin:16px;background:#2e5e3a;font-family:sans-serif;color:#fff}"
        ".g{display:grid;grid-template-columns:repeat(10,140px);gap:10px}"
        "figure{margin:0;text-align:center}figure img{width:140px;height:auto;display:block}"
        "figcaption{font-size:11px;opacity:.8}.s{width:70px;margin:2px}"
        "h2{font-size:14px;margin:12px 0 6px}</style></head><body>"
        f'<div class="g">{"".join(cells)}</div>'
        f"<h2>70px</h2><div>{small}</div></body></html>\n"
    )


def main() -> int:
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    out = os.path.join(root, "crates", "bazzica-app", "public", "carte")
    os.makedirs(out, exist_ok=True)
    total = 0
    files = {}
    for seme in SEMI:
        for n in range(1, 11):
            files[f"{seme}_{n:02d}.svg"] = carta(seme, n)
    files["dorso.svg"] = dorso()
    files["anteprima.html"] = anteprima()
    for name, data in files.items():
        with open(os.path.join(out, name), "w", encoding="utf-8", newline="\n") as fh:
            fh.write(data)
        total += len(data.encode("utf-8"))
    print(f"{len(files)} file scritti in {out} ({total} byte)")
    return 0


if __name__ == "__main__":
    sys.exit(main())

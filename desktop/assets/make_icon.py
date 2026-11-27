"""Gera o ícone do app: icon.ico (exe, atalhos e instalador) e icon-64.rgba (janela).

Desenho próprio: um "C" claro sobre um quadrado escuro de cantos redondos, com um ponto verde
(online) na abertura. Sem bibliotecas: o PNG e o ICO são escritos à mão.

Uso: python desktop/assets/make_icon.py
"""

import math
import struct
import zlib
from pathlib import Path

HERE = Path(__file__).parent
BG = (0x1E, 0x1F, 0x22)
FG = (0xDB, 0xDE, 0xE1)
DOT = (0x23, 0xA5, 0x5A)


def coverage(d):
    """Distância assinada (em pixels) para cobertura de 0 a 1, com 1 px de suavização."""
    return max(0.0, min(1.0, 0.5 - d))


def pixel(x, y, n):
    # Coordenadas de -1 a 1, no centro do pixel.
    u = (x + 0.5) / n * 2 - 1
    v = (y + 0.5) / n * 2 - 1
    px = 2 / n  # tamanho de um pixel nessas coordenadas

    # Quadrado de cantos redondos.
    r = 0.36
    qx, qy = max(abs(u) - (1 - r), 0), max(abs(v) - (1 - r), 0)
    d_bg = (math.hypot(qx, qy) - r) / px
    a_bg = coverage(d_bg)

    # Anel do "C", aberto à direita (±40°).
    ring = abs(math.hypot(u, v) - 0.48) - 0.16
    angle = abs(math.degrees(math.atan2(v, u)))
    if angle < 40:
        # Fecha o anel com pontas redondas.
        cap = math.radians(40)
        ends = [(0.48 * math.cos(cap), s * 0.48 * math.sin(cap)) for s in (1, -1)]
        ring = min(math.hypot(u - ex, v - ey) for ex, ey in ends) - 0.16
    a_fg = coverage(ring / px)

    # Ponto verde na abertura.
    a_dot = coverage((math.hypot(u - 0.52, v) - 0.13) / px)

    color = BG
    for c, a in ((FG, a_fg), (DOT, a_dot)):
        color = tuple(round(b + (f - b) * a) for b, f in zip(color, c))
    return (*color, round(255 * a_bg))


def rgba(n):
    return bytes(b for y in range(n) for x in range(n) for b in pixel(x, y, n))


def png(n):
    raw = rgba(n)
    rows = b"".join(b"\x00" + raw[y * n * 4:(y + 1) * n * 4] for y in range(n))

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", n, n, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(rows, 9))
        + chunk(b"IEND", b"")
    )


def ico(sizes):
    images = [png(n) for n in sizes]
    out = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    for n, data in zip(sizes, images):
        out += struct.pack("<BBBBHHII", n % 256, n % 256, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    return out + b"".join(images)


if __name__ == "__main__":
    (HERE / "icon.ico").write_bytes(ico([16, 24, 32, 48, 64, 128, 256]))
    (HERE / "icon-64.rgba").write_bytes(rgba(64))
    (HERE / "icon-256.png").write_bytes(png(256))
    print("icon.ico, icon-64.rgba e icon-256.png gerados")

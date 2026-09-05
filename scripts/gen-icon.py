#!/usr/bin/env python3
"""生成 FinalShell-X 应用图标主图 (1024x1024 PNG)。

设计: 蓝→青 45° 渐变圆角方卡 + 白色终端提示符 `>` 与光标下划线。
要点:
- 笔画粗、主体大,16px 任务栏下仍能辨认
- 亮色底,深色/浅色任务栏都不糊
- 与应用主题的 --accent(#4f9cf9) 同色系
用法: python scripts/gen-icon.py [输出文件=src-tauri/icons/app-icon.png]
"""
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

S = 1024      # 输出尺寸
SS = 3        # 超采样倍数(抗锯齿)
R = S * SS

C_TL = (60, 132, 255)    # 左上 蓝
C_BR = (36, 205, 220)    # 右下 青
GLYPH = (255, 255, 255)
RADIUS_RATIO = 0.225     # 圆角比例


def diagonal_gradient(size: int, c1, c2) -> Image.Image:
    """左上 -> 右下 线性渐变(逐像素精确,无条带)"""
    ax = np.linspace(0.0, 1.0, size, dtype=np.float32)
    t = (ax[None, :] + ax[:, None]) / 2.0            # (y,x) 对角插值系数
    c1 = np.array(c1, dtype=np.float32)
    c2 = np.array(c2, dtype=np.float32)
    rgb = c1[None, None, :] + (c2 - c1)[None, None, :] * t[:, :, None]
    return Image.fromarray(rgb.round().astype(np.uint8), "RGB")


def rounded_mask(size: int, radius_ratio: float) -> Image.Image:
    m = Image.new("L", (size, size), 0)
    ImageDraw.Draw(m).rounded_rectangle(
        [0, 0, size - 1, size - 1], radius=int(size * radius_ratio), fill=255
    )
    return m


def chevron(d: ImageDraw.ImageDraw, cx, cy, size, width, color):
    """粗体 `>`(圆头折线)"""
    half = size / 2
    pts = [
        (cx - half * 0.55, cy - half),
        (cx + half * 0.62, cy),
        (cx - half * 0.55, cy + half),
    ]
    d.line(pts, fill=color, width=int(width), joint="curve")
    for x, y in pts:                       # 补圆头,避免尖角发虚
        r = width / 2
        d.ellipse([x - r, y - r, x + r, y + r], fill=color)


def build() -> Image.Image:
    icon = Image.new("RGBA", (R, R), (0, 0, 0, 0))
    icon.paste(diagonal_gradient(R, C_TL, C_BR), (0, 0), rounded_mask(R, RADIUS_RATIO))

    d = ImageDraw.Draw(icon)
    chevron(d, R * 0.43, R * 0.44, R * 0.44, R * 0.09, GLYPH)
    # 光标下划线
    w, h = R * 0.30, R * 0.09
    cx, cy = R * 0.60, R * 0.68
    d.rounded_rectangle([cx - w / 2, cy - h / 2, cx + w / 2, cy + h / 2],
                        radius=h / 2, fill=(255, 255, 255, 235))
    return icon.resize((S, S), Image.LANCZOS)


if __name__ == "__main__":
    out = Path(sys.argv[1] if len(sys.argv) > 1 else "src-tauri/icons/app-icon.png")
    out.parent.mkdir(parents=True, exist_ok=True)
    build().save(out)
    print("wrote", out.resolve())

"""生成应用图标源图 (1024x1024 PNG)。

图形语义：三根竖条 = 三家 agent（Claude / Codex / WorkBuddy）的工作进度条，
高度递进表示「进度」。配色与前端 --claude / --codex / --workbuddy 一致。

用法: python scripts/make-icon.py
之后跑: npm run tauri icon src-tauri/icons/source.png
"""

from PIL import Image, ImageDraw

SIZE = 1024
BG = (23, 26, 35, 255)
BARS = [
    ((217, 119, 87), 0.46),   # claude
    ((16, 163, 127), 0.72),   # codex
    ((124, 108, 255), 0.58),  # workbuddy
]


def main() -> None:
    img = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    # 圆角底板
    d.rounded_rectangle([0, 0, SIZE - 1, SIZE - 1], radius=int(SIZE * 0.22), fill=BG)

    bar_w = int(SIZE * 0.13)
    gap = int(SIZE * 0.075)
    total = len(BARS) * bar_w + (len(BARS) - 1) * gap
    x = (SIZE - total) // 2
    base = int(SIZE * 0.76)
    radius = bar_w // 2

    for color, frac in BARS:
        h = int(SIZE * 0.52 * frac)
        top = base - h
        # 轨道
        d.rounded_rectangle(
            [x, int(SIZE * 0.24), x + bar_w, base],
            radius=radius,
            fill=(*color, 38),
        )
        # 已完成进度
        d.rounded_rectangle([x, top, x + bar_w, base], radius=radius, fill=(*color, 255))
        x += bar_w + gap

    out = "src-tauri/icons/source.png"
    img.save(out)
    print(f"已生成 {out} ({SIZE}x{SIZE})")


if __name__ == "__main__":
    main()

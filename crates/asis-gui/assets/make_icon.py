"""asis の印を作る。`python3 make_icon.py` をこのフォルダで打つと、icon.ico・icon.png・icon-64.rgba を書き直す。
画面の「自分の番のカード」の枠と同じ:暗い地に、細い 1 色の線の正方形(角は丸めない)。中に、同じ色で「AsIs」。"""
from PIL import Image, ImageDraw, ImageFont

BG = (13, 14, 13, 255)
AMBER = (242, 182, 60, 255)
FONT = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"
WORD = "AsIs"
SIZES = [16, 24, 32, 48, 64, 128, 256]


def draw(n):
    s = 8
    N = n * s
    im = Image.new("RGBA", (N, N), BG)
    d = ImageDraw.Draw(im)
    # 正方形の枠。角は丸めない。線は細く(小さい印でも 1px は残す)
    w = h = N * 0.92
    x0, y0 = (N - w) / 2, (N - h) / 2
    line = max(round(n * 0.016), 1) * s
    d.rectangle([x0, y0, x0 + w, y0 + h], outline=AMBER, width=line)
    # 枠の中の幅の 78% に収まる、いちばん大きい字
    size = N
    while True:
        font = ImageFont.truetype(FONT, size)
        l, t, r, b = d.textbbox((0, 0), WORD, font=font)
        if r - l <= (w - line * 2) * 0.78:
            break
        size -= 4
    d.text(((N - (r - l)) / 2 - l, (N - (b - t)) / 2 - t), WORD, font=font, fill=AMBER)
    return im.resize((n, n), Image.LANCZOS)


# 大きさごとに描く(縮めると、細い線が消えるため)
images = [draw(n) for n in SIZES]
images[-1].save("icon.ico", sizes=[(n, n) for n in SIZES], append_images=images[:-1])
images[-1].save("icon.png")
open("icon-64.rgba", "wb").write(draw(64).tobytes())

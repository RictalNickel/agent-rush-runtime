"""Vultr-Placeholder-Test123 pitch deck, built with Manim Slides.

Render:   ./render.sh            (or: manim-slides render -qh deck.py PitchDeck)
Present:  open out/vultr-placeholder-test123-deck.html, or: manim-slides present PitchDeck

Every number on these slides comes from the repository's docs and live evidence
(README.md, docs/DESIGN.md, docs/evidence/live/*/README.md). Do not add numbers
that are not in those files.

Layout: the frame is 14.22 x 8 units (16:9). Content stays inside x in
[-6.4, 6.4] and y in [-3.2, 3.5]; the footer sits below -3.3.
"""

from __future__ import annotations

from html import escape
from pathlib import Path

import manimpango
import numpy as np
from manim import (
    DL,
    DOWN,
    LEFT,
    RIGHT,
    UP,
    UL,
    AddTextLetterByLetter,
    AnimationGroup,
    Arrow,
    Circle,
    Create,
    DashedLine,
    Dot,
    FadeIn,
    FadeOut,
    Group,
    ImageMobject,
    LaggedStart,
    Line,
    MarkupText,
    Mobject,
    Polygon,
    Rectangle,
    RoundedRectangle,
    Text,
    UpdateFromAlphaFunc,
    VGroup,
    VMobject,
    config,
    rate_functions,
)
from manim_slides import Slide
from PIL import Image, ImageDraw

# --------------------------------------------------------------------------
# Constants
# --------------------------------------------------------------------------

PRODUCT = "Vultr-Placeholder-Test123"  # the product name may change: edit it here only
LIVE_URL = "https://144-202-59-108.sslip.io"

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
FONT_FILE = HERE / "fonts" / "Archivo[wdth,wght].ttf"
PREPARED = HERE / "media" / "prepared"

# Product UI tokens (docs/UI_DESIGN.md)
GROUND = "#12161b"
SURFACE = "#1a2027"
RAISED = "#232b34"
RULE = "#2c3540"
TEXT = "#e6eaee"
MUTED = "#9aa5b1"
FAINT = "#6f7a86"
AMBER = "#e8b04b"
OK = "#5bbf8a"
BAD = "#ec6e76"
RUN = "#6aa8e8"

SANS = "Archivo"
MONO = "DejaVu Sans Mono"

X_LEFT, X_RIGHT = -6.4, 6.4
TITLE_TOP = 3.2

config.background_color = GROUND
config.frame_rate = 30

if FONT_FILE.exists():
    manimpango.register_font(str(FONT_FILE))
else:  # fall back to a clean installed sans
    SANS = "DejaVu Sans"

# --------------------------------------------------------------------------
# Type helpers
# --------------------------------------------------------------------------


def txt(s: str, size: float = 28, color: str = TEXT, weight: str = "NORMAL", **kw) -> Text:
    return Text(s, font=SANS, font_size=size, color=color, weight=weight, **kw)


def bold(s: str, size: float = 28, color: str = TEXT, **kw) -> Text:
    return txt(s, size, color, weight="BOLD", **kw)


def display(s: str, size: float = 56, color: str = TEXT, width: int = 72) -> MarkupText:
    """Condensed heavy Archivo, as the product uses for headlines."""
    desc = f"{SANS} Bold @wdth={width}" if SANS == "Archivo" else f"{SANS} Bold"
    return MarkupText(f'<span font_desc="{desc}">{escape(s)}</span>', font_size=size, color=color)


def mono(s: str, size: float = 20, color: str = TEXT) -> Text:
    return Text(s, font=MONO, font_size=size, color=color)


def code_block(lines: list[list[tuple[str, str]]], size: float = 19) -> MarkupText:
    """One MarkupText for a code block, so indentation is kept exactly."""
    out = []
    for line in lines:
        out.append("".join(f'<span foreground="{c}">{escape(t)}</span>' for t, c in line))
    return MarkupText("\n".join(out), font=MONO, font_size=size, line_spacing=1.1)


def fit(m: Mobject, max_w: float) -> Mobject:
    if m.width > max_w:
        m.scale_to_fit_width(max_w)
    return m


def place(m: Mobject, left: float | None = None, top: float | None = None,
          right: float | None = None, bottom: float | None = None) -> Mobject:
    if left is not None:
        m.shift(RIGHT * (left - m.get_left()[0]))
    if right is not None:
        m.shift(RIGHT * (right - m.get_right()[0]))
    if top is not None:
        m.shift(UP * (top - m.get_top()[1]))
    if bottom is not None:
        m.shift(UP * (bottom - m.get_bottom()[1]))
    return m


def card(w: float, h: float, fill: str = SURFACE, stroke: str = RULE, r: float = 0.14,
         stroke_width: float = 2) -> RoundedRectangle:
    return RoundedRectangle(corner_radius=r, width=w, height=h, fill_color=fill,
                            fill_opacity=1, stroke_color=stroke, stroke_width=stroke_width)


def stack(*mobs: Mobject, buff: float = 0.12) -> VGroup:
    return VGroup(*mobs).arrange(DOWN, aligned_edge=LEFT, buff=buff)


def pair(head: str, body: str, head_size: float = 28, body_size: float = 20,
         body_color: str = MUTED, gap: float = 0.1, head_mob: Mobject | None = None) -> VGroup:
    """A bold heading over a muted line, spaced by font metrics rather than ink.

    Arranging by bounding boxes makes the gap depend on descenders; this keeps
    every heading/body pair on the same rhythm.
    """
    h = head_mob if head_mob is not None else bold(head, head_size)
    b = txt(body, body_size, body_color, line_spacing=0.9)
    ref = bold("Hg", head_size)
    place(b, left=h.get_left()[0], top=h.get_top()[1] - ref.height - gap)
    return VGroup(h, b)


def prompt_mark(side: float = 3.3) -> VGroup:
    """The product mark: an amber-edged box with a terminal prompt inside."""
    k = side / 3.3
    box = RoundedRectangle(corner_radius=0.3 * k, width=side, height=side, stroke_color=AMBER, stroke_width=5)
    inner = RoundedRectangle(corner_radius=0.18 * k, width=side - 0.7 * k, height=side - 0.7 * k,
                             stroke_color=RULE, stroke_width=2, fill_color=SURFACE, fill_opacity=1)
    chevron = mono(">", 110 * k, TEXT)
    cursor = Rectangle(width=0.42 * k, height=0.7 * k, fill_color=AMBER, fill_opacity=1, stroke_width=0)
    glyph = VGroup(chevron, cursor).arrange(RIGHT, buff=0.18 * k, aligned_edge=DOWN)
    glyph.move_to(inner)
    return VGroup(box, inner, glyph)


# --------------------------------------------------------------------------
# Icons (drawn, so they scale cleanly and match the palette)
# --------------------------------------------------------------------------


def icon_shell(s: float = 0.6, color: str = TEXT) -> VGroup:
    frame = RoundedRectangle(corner_radius=0.08 * s / 0.6, width=s, height=s * 0.8,
                             stroke_color=color, stroke_width=3)
    prompt = mono(">_", size=22 * s / 0.6, color=color).move_to(frame)
    return VGroup(frame, prompt)


def icon_browser(s: float = 0.6, color: str = TEXT) -> VGroup:
    frame = RoundedRectangle(corner_radius=0.08 * s / 0.6, width=s, height=s * 0.8,
                             stroke_color=color, stroke_width=3)
    bar = Line(frame.get_corner(UL) + DOWN * s * 0.22, frame.get_corner(UL) + DOWN * s * 0.22 + RIGHT * s,
               stroke_color=color, stroke_width=3)
    dots = VGroup(*[Dot(radius=0.025 * s / 0.6, color=color) for _ in range(3)]).arrange(RIGHT, buff=0.05 * s / 0.6)
    dots.move_to(frame.get_corner(UL) + RIGHT * s * 0.2 + DOWN * s * 0.11)
    return VGroup(frame, bar, dots)


def icon_file(s: float = 0.6, color: str = TEXT) -> VGroup:
    w, h, f = s * 0.7, s * 0.85, s * 0.22
    body = Polygon([0, 0, 0], [w - f, 0, 0], [w, -f, 0], [w, -h, 0], [0, -h, 0],
                   stroke_color=color, stroke_width=3)
    fold = VMobject(stroke_color=color, stroke_width=3).set_points_as_corners(
        [[w - f, 0, 0], [w - f, -f, 0], [w, -f, 0]])
    lines = VGroup(*[Line([s * 0.12, -h * k, 0], [w - s * 0.12, -h * k, 0], stroke_color=color, stroke_width=2.5)
                     for k in (0.5, 0.68)])
    return VGroup(body, fold, lines).move_to(np.zeros(3))


def icon_search(s: float = 0.6, color: str = TEXT) -> VGroup:
    ring = Circle(radius=s * 0.27, stroke_color=color, stroke_width=3).shift(UL * s * 0.08)
    handle = Line(ring.point_at_angle(-np.pi / 4), ring.point_at_angle(-np.pi / 4) + np.array([s * 0.22, -s * 0.22, 0]),
                  stroke_color=color, stroke_width=4)
    return VGroup(ring, handle).move_to(np.zeros(3))


def icon_convert(s: float = 0.6, color: str = TEXT) -> VGroup:
    a = Arrow(LEFT * s * 0.4 + UP * s * 0.14, RIGHT * s * 0.4 + UP * s * 0.14, buff=0, color=color,
              stroke_width=3, tip_length=0.12 * s / 0.6, max_tip_length_to_length_ratio=0.4)
    b = Arrow(RIGHT * s * 0.4 + DOWN * s * 0.14, LEFT * s * 0.4 + DOWN * s * 0.14, buff=0, color=color,
              stroke_width=3, tip_length=0.12 * s / 0.6, max_tip_length_to_length_ratio=0.4)
    return VGroup(a, b)


def icon_skill(s: float = 0.6, color: str = TEXT) -> VGroup:
    cards = VGroup(*[RoundedRectangle(corner_radius=0.05, width=s * 0.62, height=s * 0.42, stroke_color=color,
                                      stroke_width=3, fill_color=SURFACE, fill_opacity=1)
                     .shift(np.array([k * s * 0.1, -k * s * 0.1, 0])) for k in range(3)])
    return cards.move_to(np.zeros(3))


def icon_plug(s: float = 0.6, color: str = TEXT) -> VGroup:
    body = RoundedRectangle(corner_radius=0.06, width=s * 0.5, height=s * 0.34, stroke_color=color, stroke_width=3)
    prongs = VGroup(*[Line(body.get_top() + RIGHT * dx, body.get_top() + RIGHT * dx + UP * s * 0.2,
                           stroke_color=color, stroke_width=3) for dx in (-s * 0.11, s * 0.11)])
    cable = Line(body.get_bottom(), body.get_bottom() + DOWN * s * 0.24, stroke_color=color, stroke_width=3)
    return VGroup(body, prongs, cable).move_to(np.zeros(3))


def icon_sliders(s: float = 0.6, color: str = TEXT) -> VGroup:
    g = VGroup()
    for k, x in zip((0.3, -0.25, 0.1), (0.35, -0.25, 0.1)):
        y = k * s
        g.add(Line(LEFT * s * 0.4 + UP * y, RIGHT * s * 0.4 + UP * y, stroke_color=color, stroke_width=3))
        g.add(Circle(radius=s * 0.07, stroke_color=color, stroke_width=3, fill_color=SURFACE, fill_opacity=1)
              .move_to(RIGHT * x * s + UP * y))
    return g


def tick(size: float = 0.26, color: str = OK, width: float = 5) -> VMobject:
    return VMobject(stroke_color=color, stroke_width=width).set_points_as_corners(
        [[-0.5 * size, 0, 0], [-0.15 * size, -0.35 * size, 0], [0.5 * size, 0.4 * size, 0]])


def cross(size: float = 0.26, color: str = BAD, width: float = 5) -> VGroup:
    h = size / 2
    return VGroup(Line([-h, -h, 0], [h, h, 0], stroke_color=color, stroke_width=width),
                  Line([-h, h, 0], [h, -h, 0], stroke_color=color, stroke_width=width))


# --------------------------------------------------------------------------
# Images: crop, round the corners and add a hairline border
# --------------------------------------------------------------------------


def prepared(src: str, name: str, crop: tuple[int, int, int, int] | None = None,
             radius: int = 14, border: int = 2) -> str:
    PREPARED.mkdir(parents=True, exist_ok=True)
    out = PREPARED / f"{name}.png"
    im = Image.open(REPO / src).convert("RGBA")
    if crop:
        im = im.crop(crop)
    w, h = im.size
    mask = Image.new("L", (w, h), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, w - 1, h - 1), radius=radius, fill=255)
    im.putalpha(mask)
    ImageDraw.Draw(im).rounded_rectangle((0, 0, w - 1, h - 1), radius=radius, outline=RULE, width=border)
    im.save(out)
    return str(out)


def image(src: str, name: str, width: float, crop=None) -> ImageMobject:
    m = ImageMobject(prepared(src, name, crop))
    m.scale_to_fit_width(width)
    return m


# --------------------------------------------------------------------------
# Motion language
#   in:  fade + rise from slightly below; images pop from 0.85 scale
#   out: fade + rise slightly up, staggered, while the next title rises in
# --------------------------------------------------------------------------

EASE = rate_functions.ease_out_cubic
RISE = 0.28


def rise_in(*mobs: Mobject, lag: float = 0.12, run_time: float = 0.8):
    return LaggedStart(*[FadeIn(m, shift=UP * RISE, rate_func=EASE) for m in mobs],
                       lag_ratio=lag, run_time=run_time)


def pop_in(*mobs: Mobject, lag: float = 0.25, run_time: float = 0.7):
    return LaggedStart(*[FadeIn(m, scale=0.85, rate_func=EASE) for m in mobs],
                       lag_ratio=lag, run_time=run_time)


def title_in(title: Mobject, run_time: float = 0.75):
    letters = title.family_members_with_points()
    return LaggedStart(*[FadeIn(c, shift=UP * 0.18, rate_func=EASE) for c in letters],
                       lag_ratio=min(0.6, 6 / max(1, len(letters))), run_time=run_time)


# --------------------------------------------------------------------------
# The deck
# --------------------------------------------------------------------------


class PitchDeck(Slide):
    """Twelve slides; each method is one slide, and each slide is one step."""

    def construct(self) -> None:
        self.slides_in_order = [
            self.s01_title,
            self.s02_problem,
            self.s03_idea,
            self.s09_integrate,
            self.s04_architecture,
            self.s05_tools,
            self.s06_demo_kanban,
            self.s06b_demo_everyday,
            self.s07_demo_research,
            self.s08_containment,
            self.s10_vultr,
            self.s11_close,
        ]
        self.total = len(self.slides_in_order)
        self.chrome: list[Mobject] = []
        for i, slide in enumerate(self.slides_in_order, start=1):
            self.index = i
            slide()
            # A short hold renders every animation at its final state, so the
            # slide's last frame (and the PDF page) is the finished slide.
            self.wait(0.3)

    # ---- shared slide furniture -----------------------------------------

    def heading(self, eyebrow: str, title: str) -> VGroup:
        bar = Rectangle(width=0.42, height=0.07, fill_color=AMBER, fill_opacity=1, stroke_width=0)
        label = txt(eyebrow, 20, MUTED)
        eb = VGroup(bar, label).arrange(RIGHT, buff=0.18)
        place(eb, left=X_LEFT, top=3.62)
        t = display(title, 54)
        fit(t, X_RIGHT - X_LEFT)
        place(t, left=X_LEFT, top=TITLE_TOP)
        return VGroup(eb, t)

    def footer(self) -> list[Mobject]:
        name = txt(PRODUCT, 18, FAINT)
        place(name, left=X_LEFT, bottom=-3.72)
        num = txt(f"{self.index:02d} / {self.total:02d}", 18, FAINT)
        place(num, right=X_RIGHT, bottom=-3.72)
        track = Line([-config.frame_width / 2, -3.97, 0], [config.frame_width / 2, -3.97, 0],
                     stroke_color=RULE, stroke_width=4)
        frac = (self.index - 1) / (self.total - 1)
        x_end = -config.frame_width / 2 + config.frame_width * max(frac, 0.001)
        fill = Line([-config.frame_width / 2, -3.97, 0], [x_end, -3.97, 0], stroke_color=AMBER, stroke_width=4)
        return [name, num, track, fill]

    def enter(self, eyebrow: str | None, title: str | None, notes: str,
              chrome: bool = True) -> VGroup | None:
        """Start a slide: close the previous one, then bring in the new heading.

        Outgoing content fades and lifts away, staggered; the footer counter and
        progress bar update in place; the new title rises in letter by letter.
        """
        self.next_slide(notes=notes)
        old = [m for m in self.mobjects if m not in self.chrome]
        anims = []
        if old:
            anims.append(LaggedStart(*[FadeOut(m, shift=UP * RISE * 0.8) for m in old],
                                     lag_ratio=0.05, run_time=0.55))
        new_chrome = self.footer() if chrome else []
        chrome_anims = []
        if self.chrome and new_chrome:
            for a, b in zip(self.chrome, new_chrome):
                if a is self.chrome[1]:  # slide number: quick cross-fade
                    chrome_anims.append(AnimationGroup(FadeOut(a, shift=UP * 0.15), FadeIn(b, shift=UP * 0.15)))
                else:
                    chrome_anims.append(a.animate.become(b))
            new_chrome = [self.chrome[0], new_chrome[1], self.chrome[2], self.chrome[3]]
        elif new_chrome:
            # one FadeIn per piece, so each stays a top-level mobject we can track
            chrome_anims.append(AnimationGroup(*[FadeIn(m) for m in new_chrome]))
        elif self.chrome:
            chrome_anims.append(FadeOut(*self.chrome))
        head = None
        if title is not None:
            head = self.heading(eyebrow or "", title)
            bar, label = head[0]
            head_anim = AnimationGroup(
                FadeIn(bar, shift=RIGHT * 0.3, rate_func=EASE),
                FadeIn(label, shift=UP * 0.12, rate_func=EASE),
                title_in(head[1]),
                lag_ratio=0.15,
            )
            anims.append(AnimationGroup(*chrome_anims, head_anim))
        elif chrome_anims:
            anims.append(AnimationGroup(*chrome_anims))
        if anims:
            self.play(AnimationGroup(*anims, lag_ratio=0.45))
        self.chrome = new_chrome
        return head

    # ---- 1. Title ---------------------------------------------------------

    def s01_title(self) -> None:
        self.next_slide(notes=f"{PRODUCT}: plug a contained subagent into the app or agent you already have. "
                             "It does real work offsite, and the damage stays in one box.")
        name = display(PRODUCT, 150, width=70)
        fit(name, 2.75 - 0.45 - X_LEFT)  # stop short of the mark on the right
        place(name, left=X_LEFT, top=1.85)
        bar = Rectangle(width=2.2, height=0.12, fill_color=AMBER, fill_opacity=1, stroke_width=0)
        place(bar, left=X_LEFT + 0.05, top=name.get_bottom()[1] - 0.35)
        sub = stack(txt("A contained subagent for the app you already have:", 34),
                    txt("a shell and a browser, sealed in one box.", 34), buff=0.14)
        fit(sub, 2.75 - 0.45 - X_LEFT)
        place(sub, left=X_LEFT, top=bar.get_bottom()[1] - 0.45)
        meta = txt("Vultr hackathon, Blast Radius Zero track", 22, MUTED)
        place(meta, left=X_LEFT, top=sub.get_bottom()[1] - 0.5)
        # A long name is scaled down, so centre the block on the mark instead of pinning its top.
        block = VGroup(name, bar, sub, meta)
        block.shift(UP * (0.35 - block.get_center()[1]))

        # A box with an agent prompt inside: the whole idea in one mark.
        box, inner, glyph = prompt_mark(3.3)
        VGroup(box, inner, glyph).move_to([4.4, 0.35, 0])
        chevron, cursor = glyph

        self.play(title_in(name, run_time=1.0), FadeIn(inner, scale=0.85, rate_func=EASE, run_time=0.9))
        self.play(
            Create(bar, rate_func=EASE, run_time=0.5),
            Create(box, run_time=1.0),
            FadeIn(chevron, shift=UP * 0.2, rate_func=EASE, run_time=0.7),
        )
        self.play(rise_in(sub[0], sub[1], meta, lag=0.2), FadeIn(cursor, run_time=0.4))

    # ---- 2. The problem ---------------------------------------------------

    def s02_problem(self) -> None:
        self.enter("The problem", "Useful agents need a shell and a browser",
                   notes="Agents that do real work need to run code and browse. "
                         "On your own servers that is a huge blast radius: one prompt injection away from all of it.")
        agent_card = card(3.0, 1.45, fill=RAISED)
        icons = VGroup(icon_shell(0.55), icon_browser(0.55)).arrange(RIGHT, buff=0.25)
        label = bold("Your agent", 26)
        inner = VGroup(icons, label).arrange(DOWN, buff=0.18)
        agent = VGroup(agent_card, inner.move_to(agent_card)).move_to([0, -0.1, 0])

        targets_text = [
            ("API keys and tokens", (-4.65, 1.25)),
            ("Cloud metadata", (-4.65, -0.1)),
            ("Your private network", (-4.65, -1.45)),
            ("The host and its SSH", (4.65, 1.25)),
            ("Mail servers", (4.65, -0.1)),
            ("Other users' files", (4.65, -1.45)),
        ]
        targets, lines = [], []
        for s, (x, y) in targets_text:
            dot = Dot(radius=0.07, color=BAD)
            lab = txt(s, 22, TEXT)
            row = VGroup(dot, lab).arrange(RIGHT, buff=0.18)
            box = card(row.width + 0.5, 0.62)
            t = VGroup(box, row.move_to(box)).move_to([x, y, 0])
            fit(t, 3.4)
            targets.append(t)
            start = agent_card.get_left() if x < 0 else agent_card.get_right()
            start = start + UP * (y + 0.1) * 0.35
            end = t.get_right() if x < 0 else t.get_left()
            lines.append(Line(start, end, stroke_color=BAD, stroke_width=2.5, stroke_opacity=0.75))

        punch = MarkupText(
            f'<span font_family="{SANS}">One prompt injection away from </span>'
            f'<span font_family="{SANS}" foreground="{BAD}" font_weight="bold">all of it</span>'
            f'<span font_family="{SANS}">.</span>', font_size=30, color=TEXT)
        place(punch, left=X_LEFT, top=-2.4)

        self.play(pop_in(agent))
        self.play(LaggedStart(*[Create(l, rate_func=EASE) for l in lines], lag_ratio=0.12, run_time=1.0),
                  LaggedStart(*[FadeIn(t, shift=(LEFT if t.get_x() < 0 else RIGHT) * 0.3, rate_func=EASE)
                                for t in targets], lag_ratio=0.12, run_time=1.2))
        self.play(rise_in(punch))

    # ---- 3. The idea ------------------------------------------------------

    def s03_idea(self) -> None:
        self.enter("The idea", "Give every session its own sealed box",
                   notes="Each session gets one gVisor container on a private worker VM. "
                         "No credentials ever enter it, and an egress filter keeps it on the public internet.")
        box = RoundedRectangle(corner_radius=0.28, width=5.5, height=4.2, stroke_color=AMBER, stroke_width=5)
        place(box, left=X_LEFT, top=1.95)
        tab_text = txt("gVisor box, one per session", 20, GROUND, weight="BOLD")
        tab = VGroup(RoundedRectangle(corner_radius=0.1, width=tab_text.width + 0.4, height=0.42,
                                      fill_color=AMBER, fill_opacity=1, stroke_width=0), tab_text)
        tab_text.move_to(tab[0])
        tab.move_to(box.get_top() + DOWN * 0.02).align_to(box, LEFT).shift(RIGHT * 0.35)

        rows = VGroup()
        for icon, label in ((icon_shell(0.55), "Shell"), (icon_browser(0.55), "Headless browser"),
                            (icon_file(0.55), "4 GiB /workspace")):
            ic = VGroup(card(0.85, 0.85, fill=RAISED), icon)
            icon.move_to(ic[0])
            rows.add(VGroup(ic, txt(label, 26)).arrange(RIGHT, buff=0.3))
        rows.arrange(DOWN, aligned_edge=LEFT, buff=0.35)
        rows.move_to(box).shift(DOWN * 0.1).align_to(box, LEFT).shift(RIGHT * 0.5)

        points = [
            ("One sealed box per session", "gVisor, uid 1000, read-only root, no capabilities,\ncapped CPU, memory and disk"),
            ("No credentials inside", "API keys, the worker token and the inference key\nstay in trusted code"),
            ("Public internet only", "Cloud metadata, private networks, the host\nand mail ports are blocked"),
        ]
        items = VGroup()
        for head, body in points:
            items.add(pair(head, body, 30, 21, gap=0.14))
        items.arrange(DOWN, aligned_edge=LEFT, buff=0.5)
        place(items, left=0.1, top=1.9)
        fit(items, X_RIGHT - 0.1)
        place(items, left=0.1, top=1.9)

        self.play(Create(box, run_time=0.9), FadeIn(tab, shift=DOWN * 0.15, rate_func=EASE, run_time=0.6))
        self.play(pop_in(*rows, lag=0.2), rise_in(*items, lag=0.25, run_time=1.4))

    # ---- 4. Architecture --------------------------------------------------

    def node(self, head: str, sub: str, w: float, h: float, pos, head_size: float = 24) -> VGroup:
        box = card(w, h)
        content = stack(bold(head, head_size), txt(sub, 18, MUTED, line_spacing=0.85), buff=0.1)
        fit(content, w - 0.3)
        content.move_to(box)
        return VGroup(box, content).move_to([*pos, 0])

    def s04_architecture(self) -> None:
        self.enter("How it fits together", "Two VMs, one wall",
                   notes="Your app talks to the control VM, which runs the sessions and calls Vultr inference. "
                         "It never runs a command the model chose. The worker VM is private, reached over mTLS, "
                         "and runs the boxes behind the egress filter.")
        row_y = 0.75
        client = self.node("Your app", "or any MCP client", 2.1, 1.1, (-5.35, row_y))
        control = self.node("Control VM", "sessions, keys,\nspend ledger", 2.25, 1.6, (-2.6, row_y))
        infer = self.node("Vultr Serverless\nInference", "GLM 5.3 Flash", 2.25, 1.35, (-2.6, -1.55), head_size=22)

        worker_box = card(2.9, 3.6)
        worker_box.move_to([1.05, -0.1, 0])
        w_head = pair("Worker VM", "private network", 24, 18, gap=0.06)
        place(w_head, left=worker_box.get_left()[0] + 0.25, top=worker_box.get_top()[1] - 0.22)
        boxes = VGroup(*[
            VGroup(RoundedRectangle(corner_radius=0.08, width=0.68, height=0.68, stroke_color=AMBER,
                                    stroke_width=3, fill_color=RAISED, fill_opacity=1),
                   mono(">_", 16, TEXT))
            for _ in range(3)
        ]).arrange(RIGHT, buff=0.18)
        for b in boxes:
            b[1].move_to(b[0])
        boxes.move_to(worker_box).shift(DOWN * 0.05)
        w_foot = txt("one gVisor box\nper session", 17, MUTED, line_spacing=0.85)
        w_foot.next_to(boxes, DOWN, buff=0.25).align_to(w_head, LEFT)
        worker = VGroup(worker_box, w_head)

        a1 = Arrow(client.get_right(), control.get_left(), buff=0.06,
                   color=MUTED, stroke_width=3, tip_length=0.16)
        a2 = Arrow(control.get_bottom(), infer.get_top(), buff=0.06,
                   color=MUTED, stroke_width=3, tip_length=0.16)
        a2_label = txt("every model call", 17, MUTED)
        place(a2_label, right=a2.get_left()[0] - 0.15)
        a2_label.set_y(a2.get_y())
        link = DashedLine(control.get_right(), [worker_box.get_left()[0], row_y, 0], dash_length=0.1,
                          color=MUTED, stroke_width=3)
        link_label = txt("private\nmTLS", 17, MUTED, line_spacing=0.85).next_to(link, UP, buff=0.12)

        wall_x = 3.2
        wall = Line([wall_x, 1.95, 0], [wall_x, -2.3, 0], stroke_color=AMBER, stroke_width=7)
        wall_label = txt("egress filter", 20, AMBER, weight="BOLD")
        wall_label.next_to(wall, DOWN, buff=0.15)

        flows = [("Public internet", True), ("Cloud metadata", False), ("Private networks", False),
                 ("Host services, SSH", False), ("Mail ports", False)]
        ys = [1.35, 0.6, -0.15, -0.9, -1.65]
        flow_anims, label_mobs, marks = [], [], []
        for (label, allowed), y in zip(flows, ys):
            start = np.array([worker_box.get_right()[0], y, 0])
            lab = txt(label, 21, TEXT if allowed else MUTED)
            place(lab, left=wall_x + 0.62)
            lab.set_y(y)
            if allowed:
                arr = Arrow(start, [wall_x + 0.5, y, 0], buff=0.05, color=OK, stroke_width=3, tip_length=0.16)
                mark = tick(0.26).move_to([wall_x, y + 0.34, 0])
            else:
                arr = Line(start + RIGHT * 0.05, [wall_x - 0.05, y, 0], stroke_color=BAD, stroke_width=3)
                mark = cross(0.22).move_to([wall_x, y, 0])
            fit(lab, X_RIGHT - (wall_x + 0.62))
            flow_anims.append(arr)
            label_mobs.append(lab)
            marks.append(mark)
        # Left to right: the path a request takes, then the wall, then what the wall stops.
        self.play(
            pop_in(client, control, lag=0.3),
            AnimationGroup(Create(a1), Create(a2), FadeIn(a2_label, shift=UP * 0.1), pop_in(infer), lag_ratio=0.3),
            run_time=1.0,
        )
        self.play(
            Create(link), FadeIn(link_label, shift=UP * 0.1), FadeIn(worker, scale=0.9, rate_func=EASE),
            LaggedStart(*[FadeIn(b, scale=0.6, rate_func=EASE) for b in boxes], lag_ratio=0.2),
            FadeIn(w_foot, shift=UP * 0.1),
            run_time=0.8,
        )
        self.play(
            AnimationGroup(
                AnimationGroup(Create(wall, rate_func=EASE), FadeIn(wall_label, shift=UP * 0.1)),
                LaggedStart(*[AnimationGroup(Create(f), FadeIn(l, shift=RIGHT * 0.15))
                              for f, l in zip(flow_anims, label_mobs)], lag_ratio=0.12),
                LaggedStart(*[FadeIn(m, scale=1.6, rate_func=EASE) for m in marks], lag_ratio=0.15),
                lag_ratio=0.4,
            ),
            run_time=2.0,
        )

    # ---- 5. Tools ----------------------------------------------------------

    def s05_tools(self) -> None:
        self.enter("What agents can do", "Real tools, inside the box",
                   notes="These are the agent's built-in tools. Shell, browser, files, research and conversion "
                         "run in the box. Skills, its own config and MCP run in trusted code on the control VM.")
        tiles_spec = [
            (icon_shell, "Shell", "Build, test, install,\nstart servers", "box"),
            (icon_browser, "Browser", "Clicks, types and\ntakes screenshots", "box"),
            (icon_file, "Files", "Read, edit and search\nfiles in /workspace", "box"),
            (icon_search, "Web research", "Searches the web and\nreads pages as text", "box"),
            (icon_convert, "Convert", "pandoc, LibreOffice,\nImageMagick, ffmpeg", "box"),
            (icon_skill, "Skills", "Loads and saves\nSKILL.md playbooks", "control"),
            (icon_plug, "MCP", "Remote servers on\nthe operator's list", "control"),
            (icon_sliders, "Its own config", "Can tune itself,\nnot its own limits", "control"),
        ]
        w, h = 2.95, 2.05
        tiles = []
        for icon_fn, head, body, where in tiles_spec:
            bg = card(w, h)
            ic = icon_fn(0.5)
            tag = txt("in the box" if where == "box" else "trusted code", 15,
                      MUTED if where == "box" else FAINT)
            hd = bold(head, 26)
            bd = txt(body, 18, MUTED, line_spacing=0.9)
            fit(bd, w - 0.5)
            ic.move_to(bg.get_corner(UL) + np.array([0.5, -0.48, 0]))
            place(tag, right=bg.get_right()[0] - 0.22)
            tag.set_y(ic.get_y())
            place(hd, left=bg.get_left()[0] + 0.25, top=bg.get_top()[1] - 0.9)
            place(bd, left=bg.get_left()[0] + 0.25, top=hd.get_top()[1] - bold("Hg", 26).height - 0.1)
            tiles.append(VGroup(bg, ic, tag, hd, bd))
        grid = VGroup(*tiles).arrange_in_grid(rows=2, cols=4, buff=(0.33, 0.33))
        place(grid, left=X_LEFT, top=1.95)
        self.play(pop_in(*tiles, lag=0.12, run_time=1.8))

    # ---- 6. Demo: kanban ----------------------------------------------------

    def s06_demo_kanban(self) -> None:
        self.enter("Live demo, session 629d0b6f", "Build a kanban app, then use it",
                   notes="On the public instance the agent built a kanban board with a Python backend, ran it, "
                         "and used it in its own browser: add three cards, move one to Done, reload. "
                         "Then dark mode in turn two, and deleting a card in turn three. 92 tool calls, $0.0244.")
        kb = "docs/evidence/live/demo-build-kanban/outputs/"
        crop = (0, 0, 910, 450)
        shots_spec = [
            (kb + "shot3_add3.png", "kb1", "Three cards added"),
            (kb + "shot_after_reload.png", "kb2", "Moved to Done, kept after reload"),
            (kb + "dark-mode.png", "kb3", "Dark mode toggle, turn 2"),
            (kb + "after-delete.png", "kb4", "A card deleted, turn 3"),
        ]
        sw = 3.45
        shots = []
        for src, name, cap in shots_spec:
            img = image(src, name, sw, crop)
            c = txt(cap, 18, MUTED)
            fit(c, sw)
            c.next_to(img, DOWN, buff=0.14).align_to(img, LEFT)
            shots.append(Group(img, c))
        grid = Group(*shots).arrange_in_grid(rows=2, cols=2, buff=(0.3, 0.35))
        place(grid, right=X_RIGHT, top=1.95)

        col_w = grid.get_left()[0] - 0.45 - X_LEFT
        chat = image("docs/evidence/ui/demo-build-kanban.png", "kanban-chat", col_w, crop=(462, 290, 880, 515))
        place(chat, left=X_LEFT, top=1.95)
        chat_cap = txt("Its steps in the web chat", 18, MUTED).next_to(chat, DOWN, buff=0.14).align_to(chat, LEFT)

        turns = VGroup()
        for secs, what in (("166 s", "build and use"), ("82 s", "dark mode"), ("16 s", "delete")):
            turns.add(stack(display(secs, 40, width=80), txt(what, 17, MUTED), buff=0.06))
        turns.arrange(RIGHT, buff=0.55, aligned_edge=UP)
        place(turns, left=X_LEFT, top=chat_cap.get_bottom()[1] - 0.35)
        total = txt("3 turns, 92 tool calls, $0.0244 model cost", 21)
        fit(total, col_w)
        place(total, left=X_LEFT, top=turns.get_bottom()[1] - 0.3)

        self.play(pop_in(Group(chat, chat_cap)), rise_in(*turns, lag=0.2, run_time=1.0))
        self.play(pop_in(*shots, lag=0.3, run_time=1.6), rise_in(total))

    # ---- 7. Demo: everyday tasks ------------------------------------------

    def s06b_demo_everyday(self) -> None:
        self.enter("Live demo, session c8fcac86", "Data, a chart, a memo, tidy files",
                   notes="Five ordinary office requests, one turn each: totals from a CSV, a bar chart, a memo as "
                         "Word and PDF, renaming messy files, and shrinking the chart to a JPG. Every turn took under "
                         "ten seconds. 7 tool calls, $0.0034. The memo's revenue figures are right, but the model "
                         "invented the units and the softest month: it wrote them without computing.")
        ev = "docs/evidence/live/demo-everyday-tasks-v2/outputs/"
        cw = 4.5
        chart = image(ev + "revenue.png", "everyday-chart", cw)
        place(chart, right=X_RIGHT, top=1.95)
        chart_cap = txt("revenue.png, drawn by the agent with matplotlib", 18, MUTED)
        fit(chart_cap, cw)
        chart_cap.next_to(chart, DOWN, buff=0.14).align_to(chart, LEFT)
        strip = image("docs/evidence/ui/demo-everyday-tasks.png", "everyday-chat", cw, crop=(452, 128, 1000, 236))
        place(strip, left=chart.get_left()[0], top=chart_cap.get_bottom()[1] - 0.28)
        strip_cap = txt("Turn 5 in the web chat, both sizes reported", 18, MUTED)
        fit(strip_cap, cw)
        strip_cap.next_to(strip, DOWN, buff=0.14).align_to(strip, LEFT)

        col_w = chart.get_left()[0] - 0.5 - X_LEFT
        turns = [("3.9 s", "Totals per region from sales.csv", "36 rows; North 2,125,200, best month 2025-12"),
                 ("4.5 s", "Bar chart, revenue.png", "First try; the bars match the CSV totals"),
                 ("9.3 s", "Memo as .docx plus a PDF copy", "Fell back to LibreOffice when pdflatex was missing"),
                 ("2.0 s", "Rename 5 messy file names", "To lowercase-with-dashes"),
                 ("1.7 s", "PNG to an 800 px JPG", "40,020 B down to 34,193 B")]
        rows = VGroup()
        for secs, head, sub in turns:
            t = display(secs, 36, width=80)
            body = pair(head, sub, 23, 17, gap=0.05, head_mob=txt(head, 23))
            row = VGroup(t, body)
            place(body, left=t.get_left()[0] + 1.25, top=t.get_top()[1] + 0.02)
            rows.add(row)
        rows.arrange(DOWN, aligned_edge=LEFT, buff=0.26)
        fit(rows, col_w)
        place(rows, left=X_LEFT, top=1.95)

        total = txt("5 turns, 7 tool calls, $0.0034 model cost", 21)
        fit(total, col_w)
        place(total, left=X_LEFT, top=rows.get_bottom()[1] - 0.38)
        caveat = txt("The memo's revenue figures are right; its units and softest\n"
                     "month were invented by the model, never computed.", 17, MUTED, line_spacing=0.9)
        fit(caveat, col_w)
        place(caveat, left=X_LEFT, top=total.get_bottom()[1] - 0.2)

        self.play(rise_in(*rows, lag=0.18, run_time=1.4), pop_in(Group(chart, chart_cap), run_time=0.8))
        self.play(pop_in(Group(strip, strip_cap)), rise_in(total, caveat, lag=0.3))

    # ---- 8. Demo: research to PDF -----------------------------------------

    def s07_demo_research(self) -> None:
        self.enter("Live demo, session 1c047b6c", "Research, build, test, report",
                   notes="One request: research HTTP 418, build a lookup app, test it in the browser, write a PDF report. "
                         "Mid-turn I sent an interjection asking for a footer. First turn 35 seconds, "
                         "17 tool calls over two turns, model cost $0.0054.")
        steps = [("Research", "web_search, web_fetch"), ("Build", "write_file, bash"),
                 ("Browser test", "type, click, screenshot"), ("PDF report", "convert_file")]
        chips = VGroup()
        for head, sub in steps:
            g = stack(bold(head, 24), mono(sub, 14, MUTED), buff=0.08)
            bg = card(2.55, 0.95)
            fit(g, 2.12)
            g.move_to(bg)
            place(g, left=bg.get_left()[0] + 0.22)
            chips.add(VGroup(bg, g))
        chips.arrange(RIGHT, buff=0.87)
        place(chips, left=X_LEFT, top=2.05)
        arrows = VGroup(*[Arrow(a.get_right(), b.get_left(), buff=0.1, color=AMBER, stroke_width=3, tip_length=0.15)
                          for a, b in zip(chips[:-1], chips[1:])])

        chat = image("docs/evidence/ui/live-session.png", "research-chat", 5.5, crop=(450, 262, 1236, 752))
        place(chat, left=X_LEFT, top=0.75)
        chat_cap = txt("Follow-up turn in the web chat, with the PDF attached", 18, MUTED)
        chat_cap.next_to(chat, DOWN, buff=0.14).align_to(chat, LEFT)

        app = image("docs/evidence/live/session-demo-v2/screenshot.png", "research-app", 6.0, crop=(310, 18, 980, 278))
        place(app, right=X_RIGHT, top=0.75)
        app_cap = txt("The app, as the agent's browser saw it", 18, MUTED)
        app_cap.next_to(app, DOWN, buff=0.14).align_to(app, LEFT)

        stats = VGroup()
        for big, small in (("35 s", "first turn"), ("17", "tool calls, two turns"), ("$0.0054", "model cost")):
            stats.add(stack(display(big, 52, width=80), txt(small, 18, MUTED), buff=0.08))
        stats.arrange(RIGHT, buff=0.6, aligned_edge=UP)
        fit(stats, X_RIGHT - app.get_left()[0])
        place(stats, left=app.get_left()[0], top=app_cap.get_bottom()[1] - 0.35)

        self.play(LaggedStart(*[AnimationGroup(FadeIn(c, shift=UP * RISE, rate_func=EASE),
                                               *([Create(arrows[i - 1])] if i else []))
                                for i, c in enumerate(chips)], lag_ratio=0.3, run_time=1.4))
        self.play(pop_in(Group(chat, chat_cap), Group(app, app_cap), lag=0.35, run_time=1.1))
        self.play(rise_in(*stats, lag=0.2))

    # ---- 9. Containment proof ---------------------------------------------

    def s08_containment(self) -> None:
        self.enter("Containment, tested live", "We tried to break out",
                   notes="A probe session asked the agent to reach metadata, the worker, the host's SSH and mail ports. "
                         "Every attempt failed at the network. A 4300 MiB write stopped at the workspace quota. "
                         "On the deployed binary, 9 of 9 box checks and 10 of 10 job checks pass.")
        # Left: probe attempts, each struck through with a red cross.
        probes = [("Cloud metadata, 169.254.169.254", "No route to host"),
                  ("Worker listener, 10.42.0.4:8443", "Could not connect"),
                  ("Host SSH, port 22", "Could not connect"),
                  ("SMTP, ports 25 and 587", "Network is unreachable")]
        header_l = txt("Probe session 2401b687, real tool calls", 19, MUTED)
        place(header_l, left=X_LEFT, top=2.0)
        rows, crosses = VGroup(), []
        for head, result in probes:
            text = pair(head, result, 23, 18, BAD, gap=0.04, head_mob=txt(head, 23))
            mark = cross(0.26)
            row = VGroup(mark, text).arrange(RIGHT, buff=0.28)
            rows.add(row)
            crosses.append(mark)
        rows.arrange(DOWN, aligned_edge=LEFT, buff=0.24)
        place(rows, left=X_LEFT + 0.05, top=header_l.get_bottom()[1] - 0.3)

        disk_label = txt("dd 4300 MiB into /workspace", 23)
        place(disk_label, left=X_LEFT, top=rows.get_bottom()[1] - 0.42)
        bar_w = 6.0
        track = RoundedRectangle(corner_radius=0.08, width=bar_w, height=0.3, fill_color=RAISED, fill_opacity=1,
                                 stroke_color=RULE, stroke_width=2)
        place(track, left=X_LEFT, top=disk_label.get_bottom()[1] - 0.18)
        quota_x = X_LEFT + bar_w * 4096 / 4300
        quota = Line([quota_x, track.get_top()[1] + 0.12, 0], [quota_x, track.get_bottom()[1] - 0.12, 0],
                     stroke_color=AMBER, stroke_width=4)
        quota_label = txt("4096 MiB quota", 17, AMBER)
        place(quota_label, right=quota_x + 0.1, bottom=track.get_top()[1] + 0.16)
        stop_frac = 4126887936 / (4300 * 1024 * 1024)
        fill = Rectangle(width=bar_w * stop_frac, height=0.3, fill_color=RUN, fill_opacity=1, stroke_width=0)
        place(fill, left=X_LEFT, top=track.get_top()[1])
        fill.stretch_to_fit_height(0.26).align_to(track, DOWN).shift(UP * 0.02)
        disk_result = txt("No space left on device, after 4,126,887,936 bytes", 18, BAD)
        place(disk_result, left=X_LEFT, top=track.get_bottom()[1] - 0.15)

        # Right: qualification checks ticking in.
        col_x = 0.9
        box_checks = ["Metadata", "Private networks", "Host services", "SMTP", "Public HTTPS",
                      "No credentials", "Read-only root", "Disk quota", "Volume destroyed"]
        counter_box = bold("0/9", 64, OK)
        place(counter_box, left=col_x, top=2.05)
        box_lab = txt("box checks pass", 24, TEXT)
        final_box = place(bold("9/9", 64, OK), left=col_x, top=2.05)
        box_lab.next_to(final_box, RIGHT, buff=0.3).align_to(final_box, DOWN).shift(UP * 0.08)
        cells, ticks = [], []
        for name in box_checks:
            t = tick(0.22)
            l = txt(name, 17, MUTED)
            cell = VGroup(t, l).arrange(RIGHT, buff=0.12)
            cells.append(cell)
            ticks.append(t)
        grid = VGroup(*cells).arrange_in_grid(rows=3, cols=3, buff=(0.3, 0.26), col_alignments="lll")
        fit(grid, X_RIGHT - col_x)
        place(grid, left=col_x, top=counter_box.get_bottom()[1] - 0.3)

        counter_job = bold("0/10", 64, OK)
        place(counter_job, left=col_x, top=grid.get_bottom()[1] - 0.55)
        job_lab = txt("job checks pass", 24, TEXT)
        final_job = place(bold("10/10", 64, OK), left=col_x, top=counter_job.get_top()[1])
        job_lab.next_to(final_job, RIGHT, buff=0.3).align_to(final_job, DOWN).shift(UP * 0.08)
        dots = VGroup(*[VGroup(Circle(radius=0.2, stroke_color=RULE, stroke_width=3, fill_color=SURFACE,
                                      fill_opacity=1), tick(0.18, width=4)) for _ in range(10)]).arrange(RIGHT, buff=0.14)
        place(dots, left=col_x, top=counter_job.get_bottom()[1] - 0.3)
        fit(dots, X_RIGHT - col_x)

        def counter_updater(mob, total, anchor):
            def update(m, alpha):
                n = int(round(alpha * total))
                new = bold(f"{n}/{total}", 64, OK)
                place(new, left=anchor[0], top=anchor[1])
                m.become(new)
            return update

        cells_static = [c[1] for c in cells]
        self.play(FadeIn(header_l, shift=UP * 0.1),
                  LaggedStart(*[FadeIn(r[1], shift=UP * RISE, rate_func=EASE) for r in rows], lag_ratio=0.15),
                  FadeIn(counter_box, box_lab, counter_job, job_lab, *cells_static, shift=UP * 0.15,
                         rate_func=EASE),
                  FadeIn(*[d[0] for d in dots]),
                  run_time=0.9)
        # Attempts fail one after another, the disk write runs into its quota,
        # and then both qualification reports tick in together.
        self.play(LaggedStart(*[FadeIn(c, scale=1.8, rate_func=EASE) for c in crosses], lag_ratio=0.25),
                  FadeIn(disk_label, track, shift=UP * 0.1), Create(quota), FadeIn(quota_label),
                  run_time=0.8)
        fill_target = fill.copy()
        fill.stretch_to_fit_width(0.01).align_to(track, LEFT)
        self.add(fill)
        self.play(fill.animate.become(fill_target), run_time=0.8, rate_func=rate_functions.ease_in_out_sine)
        self.play(fill.animate.set_fill(BAD), FadeIn(disk_result, shift=UP * 0.1), run_time=0.3)
        anchor_box = (counter_box.get_left()[0], counter_box.get_top()[1])
        anchor_job = (counter_job.get_left()[0], counter_job.get_top()[1])
        self.play(
            UpdateFromAlphaFunc(counter_box, counter_updater(counter_box, 9, anchor_box)),
            LaggedStart(*[AnimationGroup(Create(t), c[1].animate.set_color(TEXT)) for t, c in zip(ticks, cells)],
                        lag_ratio=0.3),
            UpdateFromAlphaFunc(counter_job, counter_updater(counter_job, 10, anchor_job)),
            LaggedStart(*[AnimationGroup(d[0].animate.set_stroke(OK).set_fill(RAISED), Create(d[1]))
                          for d in dots], lag_ratio=0.3),
            run_time=1.5,
        )

    # ---- 10. Integrate -------------------------------------------------------

    def s09_integrate(self) -> None:
        self.enter("Plug it in", "A sealed subagent for your app",
                   notes="This is the point of it: plug it into the app or agent you already have. "
                         "Add it to Claude Code, Cursor or any MCP client as one remote server, or start a session "
                         "with one POST. Your app hands off the task; the subagent runs code and a browser offsite, "
                         "in its own box on Vultr, and hands back the result. Turn ends are POSTed to your webhooks, signed.")
        C_CMD, C_FLAG, C_STR, C_VAR, C_DIM = TEXT, MUTED, OK, RUN, FAINT
        rest = code_block([
            [("curl ", C_CMD), ("$HOST", C_VAR), ("/v1/sessions \\", C_CMD)],
            [("  -H ", C_FLAG), ('"authorization: Bearer ', C_STR), ("$AR_KEY", C_VAR), ('"', C_STR), (" \\", C_CMD)],
            [("  -H ", C_FLAG), ("'content-type: application/json'", C_STR), (" \\", C_CMD)],
            [("  -d ", C_FLAG), ("'{\"message\":\"Build a kanban board and test it\"}'", C_STR)],
        ])
        mcp = code_block([
            [("claude mcp add ", C_CMD), ("--transport http ", C_FLAG), ("agent \\", C_CMD)],
            [("  https://<host>/mcp \\", C_VAR)],
            [("  --header ", C_FLAG), ('"Authorization: Bearer ar_..."', C_STR)],
        ])
        panel_w = 8.3

        def panel(label: str, code: MarkupText) -> VGroup:
            lab = txt(label, 18, MUTED)
            fit(code, panel_w - 0.6)
            bg = card(panel_w, code.height + 0.95)
            place(lab, left=bg.get_left()[0] + 0.3, top=bg.get_top()[1] - 0.2)
            place(code, left=bg.get_left()[0] + 0.3, top=lab.get_bottom()[1] - 0.2)
            return VGroup(bg, lab, code)

        p1 = panel("From any app: one POST hands a subagent its task", rest)
        p2 = panel("From your agent: Claude Code, Cursor or any MCP client", mcp)
        panels = VGroup(p1, p2).arrange(DOWN, buff=0.35, aligned_edge=LEFT)
        place(panels, left=X_LEFT, top=1.95)

        feats = [("Offsite, on Vultr", "Its own box, never your servers"),
                 ("MCP server at /mcp", "15 tools, the same API keys"),
                 ("REST and live events", "Sessions, files, server-sent events"),
                 ("Signed webhooks", "HMAC-SHA256 on every turn end")]
        items = VGroup()
        for head, sub in feats:
            dot = Rectangle(width=0.07, height=0.62, fill_color=AMBER, fill_opacity=1, stroke_width=0)
            body = pair(head, sub, 24, 18, gap=0.06)
            items.add(VGroup(dot, body).arrange(RIGHT, buff=0.22))
        items.arrange(DOWN, aligned_edge=LEFT, buff=0.42)
        place(items, left=panels.get_right()[0] + 0.5, top=1.95)
        fit(items, X_RIGHT - items.get_left()[0])

        self.play(FadeIn(p1[0], p1[1], shift=UP * RISE, rate_func=EASE), run_time=0.5)
        self.play(AddTextLetterByLetter(p1[2], time_per_char=0.012, run_time=1.2),
                  rise_in(*items, lag=0.25, run_time=1.4))
        self.play(FadeIn(p2[0], p2[1], shift=UP * RISE, rate_func=EASE), run_time=0.45)
        self.play(AddTextLetterByLetter(p2[2], time_per_char=0.012, run_time=0.9))

    # ---- 11. Vultr ----------------------------------------------------------

    def s10_vultr(self) -> None:
        self.enter("Built on Vultr", "Every agent call runs on Vultr",
                   notes="Every agent model call goes to Vultr Serverless Inference, GLM 5.3 Flash. "
                         "Both VMs are Vultr instances in Chicago. The 16 vCPU worker has run "
                         "72 agent sessions at once through the public URL. "
                         "These are the real model costs of the demos.")
        rows = VGroup()
        for icon_text, head, sub in (
            ("AI", "Vultr Serverless Inference", "Every agent model call, GLM 5.3 Flash"),
            ("VM", "Two Vultr VMs in Chicago", "Control 2 vCPU, 4 GB. Private worker 16 vCPU, 64 GB: 72 agent sessions at once"),
        ):
            badge = VGroup(card(0.9, 0.9, fill=RAISED), bold(icon_text, 24, TEXT))
            badge[1].move_to(badge[0])
            body = pair(head, sub, 30, 21, gap=0.1)
            rows.add(VGroup(badge, body).arrange(RIGHT, buff=0.35))
        rows.arrange(DOWN, aligned_edge=LEFT, buff=0.38)
        place(rows, left=X_LEFT, top=1.95)

        cap = txt("Model cost of real runs on the public instance", 19, MUTED)
        place(cap, left=X_LEFT, top=rows.get_bottom()[1] - 0.5)
        costs = [("$0.0244", "Build and use a kanban app", "92 tool calls, 3 turns"),
                 ("$0.0054", "Research, build, test, PDF", "17 tool calls, 2 turns"),
                 ("$0.0058", "One-shot job 7d466fa9", "5 model calls, 58 s")]
        cards = VGroup()
        cw = (X_RIGHT - X_LEFT - 2 * 0.35) / 3
        for big, head, sub in costs:
            bg = card(cw, 1.9)
            content = stack(display(big, 60, width=80), txt(head, 21), txt(sub, 18, MUTED), buff=0.1)
            fit(content, cw - 0.5)
            place(content, left=bg.get_left()[0] + 0.3)
            content.set_y(bg.get_y())
            cards.add(VGroup(bg, content))
        cards.arrange(RIGHT, buff=0.35)
        place(cards, left=X_LEFT, top=cap.get_bottom()[1] - 0.2)

        self.play(rise_in(*rows, lag=0.3, run_time=1.0))
        self.play(FadeIn(cap, shift=UP * 0.1), pop_in(*cards, lag=0.2, run_time=1.2))

    # ---- 12. Close ----------------------------------------------------------

    def s11_close(self) -> None:
        self.enter(None, None, chrome=False,
                   notes="It is live now. Plug it into your app, or open the URL and give it a task.")
        name = display(PRODUCT, 120, width=70)
        fit(name, 3.1 - 0.45 - X_LEFT)  # stop short of the mark on the right
        line = txt("Plug a contained subagent into any app.", 34)
        line2 = txt("A blast radius of one box.", 34, MUTED)
        lines = stack(line, line2, buff=0.14)
        fit(lines, 3.1 - 0.45 - X_LEFT)
        url_lab = txt("Try it live", 20, MUTED)
        url = mono(LIVE_URL, 30, AMBER)
        url_group = stack(url_lab, url, buff=0.14)
        group = stack(name, lines, url_group, buff=0.55)
        place(group, left=X_LEFT, top=1.95)
        bar = Rectangle(width=1.8, height=0.1, fill_color=AMBER, fill_opacity=1, stroke_width=0)
        place(bar, left=X_LEFT + 0.05, top=name.get_bottom()[1] - 0.2)
        lines.shift(DOWN * 0.15)
        url_group.shift(DOWN * 0.15)
        block = VGroup(name, bar, lines, url_group)
        block.shift(UP * (0.0 - block.get_center()[1]))

        box, inner, glyph = prompt_mark(3.0)
        VGroup(box, inner, glyph).move_to([4.6, 0.0, 0])

        self.play(title_in(name, run_time=0.9), FadeIn(inner, glyph, scale=0.85, rate_func=EASE, run_time=0.8))
        self.play(Create(bar, run_time=0.4), Create(box, run_time=0.9), rise_in(line, line2, lag=0.25))
        self.play(rise_in(url_lab, url, lag=0.2, run_time=0.7))

"""Boxoro pitch deck, built with Manim Slides.

Render:   ./render.sh            (or: manim-slides render -qh deck.py PitchDeck)
Present:  open out/boxoro-deck.html, or: manim-slides present PitchDeck

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
    GrowArrow,
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

PRODUCT = "Boxoro"  # the product name may change: edit it here only
LIVE_URL = "https://144-202-59-108.sslip.io"
# Live result of qa/containment_demo.py on the public instance, 2026-11-07: (blocked, attempted).
CONTAINMENT = (8, 8)

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
    """Eight slides; each method is one slide, and each slide is one step."""

    def construct(self) -> None:
        self.slides_in_order = [
            self.s1_title,
            self.s2_problem,
            self.s3_plug_in,
            self.s4_box,
            self.s5_demo,
            self.s6_containment,
            self.s7_vultr,
            self.s8_close,
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

    def s1_title(self) -> None:
        self.next_slide(notes=f"{PRODUCT} gives the app or agent you already have a contained subagent. "
                              "It does real work offsite, and anything that goes wrong stays in one box.")
        name = display(PRODUCT, 150, width=70)
        fit(name, 2.75 - 0.45 - X_LEFT)  # stop short of the mark on the right
        place(name, left=X_LEFT, top=1.85)
        bar = Rectangle(width=2.2, height=0.12, fill_color=AMBER, fill_opacity=1, stroke_width=0)
        place(bar, left=X_LEFT + 0.05, top=name.get_bottom()[1] - 0.35)
        sub = stack(txt("A contained subagent", 44),
                    txt("for the app you already have.", 44), buff=0.16)
        fit(sub, 2.75 - 0.45 - X_LEFT)
        place(sub, left=X_LEFT, top=bar.get_bottom()[1] - 0.45)
        meta = txt("Vultr hackathon, Blast Radius Zero track", 22, MUTED)
        place(meta, left=X_LEFT, top=sub.get_bottom()[1] - 0.5)
        # A long name is scaled down, so centre the block on the mark instead of pinning its top.
        block = VGroup(name, bar, sub, meta)
        block.shift(UP * (0.35 - block.get_center()[1]))

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

    def s2_problem(self) -> None:
        self.enter("The problem", "Agents that run code are risky",
                   notes="Useful agents run code and drive a browser. Run that on your own servers and "
                         "one prompt injection can reach your keys, your network and your host.")
        agent_card = card(3.2, 1.7, fill=RAISED)
        icons = VGroup(icon_shell(0.7), icon_browser(0.7)).arrange(RIGHT, buff=0.3)
        label = bold("Your agent", 30)
        inner = VGroup(icons, label).arrange(DOWN, buff=0.22)
        agent = VGroup(agent_card, inner.move_to(agent_card)).move_to([0, 0.15, 0])

        targets_text = [("Your API keys", (-4.5, 1.05)), ("Your network", (-4.5, -0.75)),
                        ("Your host", (4.5, 1.05)), ("Your users' data", (4.5, -0.75))]
        targets, lines = [], []
        for s, (x, y) in targets_text:
            dot = Dot(radius=0.09, color=BAD)
            row = VGroup(dot, txt(s, 30, TEXT)).arrange(RIGHT, buff=0.22)
            box = card(3.6, 0.95)
            targets.append(VGroup(box, row.move_to(box)).move_to([x, y, 0]))
            start = agent_card.get_left() if x < 0 else agent_card.get_right()
            start = start + UP * y * 0.35
            end = targets[-1].get_right() if x < 0 else targets[-1].get_left()
            lines.append(Line(start, end, stroke_color=BAD, stroke_width=3, stroke_opacity=0.75))

        punch = MarkupText(
            f'<span font_family="{SANS}">One prompt injection away from </span>'
            f'<span font_family="{SANS}" foreground="{BAD}" font_weight="bold">all of it</span>'
            f'<span font_family="{SANS}">.</span>', font_size=40, color=TEXT)
        place(punch, left=X_LEFT, top=-2.2)

        self.play(pop_in(agent))
        self.play(LaggedStart(*[Create(l, rate_func=EASE) for l in lines], lag_ratio=0.12, run_time=1.0),
                  LaggedStart(*[FadeIn(t, shift=(LEFT if t.get_x() < 0 else RIGHT) * 0.3, rate_func=EASE)
                                for t in targets], lag_ratio=0.12, run_time=1.2))
        self.play(rise_in(punch))

    # ---- 3. Plug it in ----------------------------------------------------

    def s3_plug_in(self) -> None:
        self.enter("The fix", "Hand the risky work to a subagent",
                   notes="Plug it into the app or agent you already have, over MCP or one REST call. "
                         "The subagent does the work in its own box on Vultr and hands back the result.")
        y = 0.55
        app_card = card(3.4, 2.0, fill=RAISED)
        app_text = stack(bold("Your app", 32), txt("or Claude Code,\nCursor, any agent", 22, MUTED, line_spacing=0.9),
                         buff=0.14)
        app = VGroup(app_card, app_text.move_to(app_card)).move_to([X_LEFT + 1.7, y, 0])

        box = RoundedRectangle(corner_radius=0.25, width=4.0, height=2.5, stroke_color=AMBER, stroke_width=5)
        box.move_to([X_RIGHT - 2.0, y, 0])
        tab_text = txt("subagent box on Vultr", 18, GROUND, weight="BOLD")
        tab = VGroup(RoundedRectangle(corner_radius=0.1, width=tab_text.width + 0.4, height=0.4,
                                      fill_color=AMBER, fill_opacity=1, stroke_width=0), tab_text)
        tab_text.move_to(tab[0])
        tab.move_to(box.get_top()).align_to(box, LEFT).shift(RIGHT * 0.3)
        icons = VGroup()
        for ic, lab in ((icon_shell(0.6), "Shell"), (icon_browser(0.6), "Browser"), (icon_file(0.6), "Files")):
            icons.add(VGroup(ic, txt(lab, 20, MUTED)).arrange(DOWN, buff=0.15))
        icons.arrange(RIGHT, buff=0.45).move_to(box).shift(DOWN * 0.1)

        x0, x1 = app_card.get_right()[0] + 0.15, box.get_left()[0] - 0.15
        go = Arrow([x0, y + 0.4, 0], [x1, y + 0.4, 0], buff=0, color=AMBER, stroke_width=5, tip_length=0.25)
        back = Arrow([x1, y - 0.4, 0], [x0, y - 0.4, 0], buff=0, color=OK, stroke_width=5, tip_length=0.25)
        go_lab = txt("a task, over MCP or REST", 22, TEXT).next_to(go, UP, buff=0.15)
        back_lab = txt("results and files", 22, OK).next_to(back, DOWN, buff=0.15)

        cmd = code_block([[("claude mcp add ", TEXT), ("--transport http ", MUTED), ("agent ", TEXT),
                           ("https://<host>/mcp", RUN)]], size=24)
        cmd_bg = card(X_RIGHT - X_LEFT, cmd.height + 0.6)
        fit(cmd, cmd_bg.width - 0.6)
        place(cmd_bg, left=X_LEFT, top=-1.55)
        cmd.move_to(cmd_bg)
        place(cmd, left=X_LEFT + 0.35)
        cmd_cap = txt("One line in Claude Code. Or one POST /v1/sessions from any app.", 22, MUTED)
        place(cmd_cap, left=X_LEFT, top=cmd_bg.get_bottom()[1] - 0.2)

        self.play(pop_in(app), Create(box, run_time=0.8), FadeIn(tab, shift=DOWN * 0.15, rate_func=EASE),
                  pop_in(*icons, lag=0.15))
        self.play(GrowArrow(go), FadeIn(go_lab, shift=UP * 0.1), run_time=0.7)
        self.play(GrowArrow(back), FadeIn(back_lab, shift=DOWN * 0.1), run_time=0.7)
        self.play(FadeIn(cmd_bg, shift=UP * RISE, rate_func=EASE), run_time=0.4)
        self.play(AddTextLetterByLetter(cmd, time_per_char=0.015, run_time=0.9), rise_in(cmd_cap))

    # ---- 4. The box -------------------------------------------------------

    def s4_box(self) -> None:
        self.enter("The box", "Real tools inside, solid walls around",
                   notes="Each session gets its own gVisor box with a shell, a browser and files. "
                         "Nothing secret is inside, it cannot reach your network, and every resource is capped.")
        box = RoundedRectangle(corner_radius=0.28, width=5.2, height=4.4, stroke_color=AMBER, stroke_width=6)
        place(box, left=X_LEFT, top=1.9)
        tab_text = txt("one gVisor box per session", 20, GROUND, weight="BOLD")
        tab = VGroup(RoundedRectangle(corner_radius=0.1, width=tab_text.width + 0.4, height=0.42,
                                      fill_color=AMBER, fill_opacity=1, stroke_width=0), tab_text)
        tab_text.move_to(tab[0])
        tab.move_to(box.get_top()).align_to(box, LEFT).shift(RIGHT * 0.35)
        rows = VGroup()
        for icon, label in ((icon_shell(0.7), "Shell"), (icon_browser(0.7), "Browser"), (icon_file(0.7), "Files")):
            ic = VGroup(card(1.0, 1.0, fill=RAISED), icon)
            icon.move_to(ic[0])
            rows.add(VGroup(ic, txt(label, 34)).arrange(RIGHT, buff=0.35))
        rows.arrange(DOWN, aligned_edge=LEFT, buff=0.32)
        rows.move_to(box).shift(DOWN * 0.1).align_to(box, LEFT).shift(RIGHT * 0.55)

        walls = ["No keys or secrets inside", "No private network or metadata",
                 "No way to the host or other boxes", "Capped memory, disk, time, spend"]
        items = VGroup()
        for w in walls:
            items.add(VGroup(cross(0.3), txt(w, 30)).arrange(RIGHT, buff=0.3))
        items[-1][0].become(tick(0.34).move_to(items[-1][0]))
        items.arrange(DOWN, aligned_edge=LEFT, buff=0.55)
        place(items, left=box.get_right()[0] + 0.6, top=1.55)
        fit(items, X_RIGHT - items.get_left()[0])

        self.play(Create(box, run_time=0.9), FadeIn(tab, shift=DOWN * 0.15, rate_func=EASE, run_time=0.6))
        self.play(pop_in(*rows, lag=0.2), rise_in(*items, lag=0.25, run_time=1.4))

    # ---- 5. Live demo -----------------------------------------------------

    def s5_demo(self) -> None:
        self.enter("Live demo", "It built an app, then used it",
                   notes="On the public instance the agent built a kanban board, ran it, and tested it in its own "
                         "browser over three turns. 92 tool calls for $0.0244 of model time.")
        shot = image("docs/evidence/ui/demo-build-kanban.png", "kanban-ui", 8.2, crop=(452, 140, 1232, 575))
        place(shot, right=X_RIGHT, top=1.95)
        stats = VGroup()
        for big, small in (("3", "turns"), ("92", "tool calls"), ("$0.0244", "model cost")):
            stats.add(stack(display(big, 72, width=80), txt(small, 24, MUTED), buff=0.08))
        stats.arrange(DOWN, aligned_edge=LEFT, buff=0.42)
        fit(stats, shot.get_left()[0] - 0.5 - X_LEFT)
        place(stats, left=X_LEFT, top=1.95)
        cap = txt("The agent's reply and its own screenshot, session 629d0b6f", 20, MUTED)
        fit(cap, shot.width)
        cap.next_to(shot, DOWN, buff=0.16).align_to(shot, LEFT)

        self.play(pop_in(Group(shot, cap), run_time=0.9), rise_in(*stats, lag=0.25, run_time=1.2))

    # ---- 6. Containment ---------------------------------------------------

    def s6_containment(self) -> None:
        blocked, total = CONTAINMENT
        self.enter("Containment, tested live", "We tried to break out",
                   notes=f"From inside a live box we tried eight escapes: stealing cloud credentials, finding keys, "
                         f"reaching the private network and the host, spam, system files, a fork bomb and an 8 GiB "
                         f"memory grab. {blocked} of {total} were blocked. The fork bomb and the memory grab only killed that "
                         f"box, which restarted on the next call with its files kept.")
        big = display(f"{blocked}/{total}", 190, OK, width=80)
        place(big, left=X_LEFT, top=1.95)
        big_lab = txt("escape attempts\nblocked", 36, line_spacing=0.9)
        place(big_lab, left=X_LEFT + 0.05, top=big.get_bottom()[1] - 0.3)
        checks = txt("A killed box restarts on the next call,\nits files kept. The internet still works.", 22,
                     MUTED, line_spacing=0.9)
        place(checks, left=X_LEFT + 0.05, top=big_lab.get_bottom()[1] - 0.5)

        attempts = ["Steal cloud credentials", "Find API keys in its env", "Reach the private network",
                    "SSH into the host", "Send spam on port 25", "Write to system files",
                    "Fork bomb", "Grab 8 GiB of memory"]
        rows, marks = VGroup(), []
        for a in attempts:
            m = cross(0.28)
            rows.add(VGroup(m, txt(a, 30)).arrange(RIGHT, buff=0.32))
            marks.append(m)
        rows.arrange(DOWN, aligned_edge=LEFT, buff=0.3)
        place(rows, left=0.6, top=1.95)
        for r in rows:
            r[0].set_opacity(0)

        self.play(rise_in(*[r[1] for r in rows], lag=0.12, run_time=1.0))
        self.play(LaggedStart(*[m.animate.set_opacity(1) for m in marks], lag_ratio=0.2, run_time=1.2),
                  FadeIn(big, scale=0.85, rate_func=EASE, run_time=0.8))
        self.play(rise_in(big_lab, checks, lag=0.3))

    # ---- 7. Vultr ---------------------------------------------------------

    def s7_vultr(self) -> None:
        self.enter("Built on Vultr", "Every agent call runs on Vultr",
                   notes="Every agent model call goes to Vultr Serverless Inference, GLM 5.3 Flash. Both VMs run on "
                         "Vultr in Chicago: a public control VM and a private 16 vCPU worker, which has run "
                         "72 agent sessions at once.")
        cards_spec = [("GLM 5.3 Flash", "Vultr Serverless Inference,\nfor every agent call"),
                      ("72", "agent sessions at once on\na 16 vCPU / 64 GB Vultr VM"),
                      ("2 VMs", "in Chicago: a public control\nVM and a private worker")]
        cw = (X_RIGHT - X_LEFT - 2 * 0.35) / 3
        cards = VGroup()
        for big, sub in cards_spec:
            bg = card(cw, 3.6)
            head = fit(display(big, 80, width=78), cw - 0.6)
            content = stack(head, fit(txt(sub, 24, MUTED, line_spacing=0.95), cw - 0.6), buff=0.3)
            place(content, left=bg.get_left()[0] + 0.3)
            content.set_y(bg.get_y())
            cards.add(VGroup(bg, content))
        cards.arrange(RIGHT, buff=0.35)
        place(cards, left=X_LEFT, top=1.6)
        self.play(pop_in(*cards, lag=0.25, run_time=1.4))

    # ---- 8. Close ---------------------------------------------------------

    def s8_close(self) -> None:
        self.enter(None, None, chrome=False,
                   notes="It is live now. Plug it into your app, or open the URL and give it a task.")
        name = display(PRODUCT, 120, width=70)
        fit(name, 3.1 - 0.45 - X_LEFT)  # stop short of the mark on the right
        line = txt("A contained subagent for any app.", 38)
        line2 = txt("A blast radius of one box.", 38, MUTED)
        lines = stack(line, line2, buff=0.16)
        fit(lines, 3.1 - 0.45 - X_LEFT)
        url_lab = txt("Try it live", 22, MUTED)
        url = mono(LIVE_URL, 32, AMBER)
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

# progress_oracle.py SCENE.qml [hide=MS show=MS] [now=MS] GRAB_MS...
# Shows the scene (an Item root with running Material ProgressBars) in a QQuickView and prints
# the window as Qt drew it GRAB_MS after its first frame, each with the times at which Qt had
# rendered: the fixtures crates/shell/tests/progress_bar_qt/*.txt.
#
# QQuickMaterialProgressBarNode keeps its own QElapsedTimer: started in the sync of the first
# frame that finds the bar running, read in whole milliseconds before each render, and started
# again from zero by the first render at or past 1760 ms (QQuickAnimatedNode::advance). So a
# frame is a function of the renders before it. Per frame this prints
#     frame MS...
# the milliseconds from the first sync to the render that started the timer again (if one
# did) and to the render that was grabbed, then the rows that are not all background as
#     Y0-Y1: X0-X1:RRGGBB ...
# A frame whose millisecond could be either of two (the timer started somewhere inside the
# first sync) is left out.
#
# With `hide=MS show=MS` the root's children are hidden and shown again at those times. A
# frame grabbed while they are hidden is printed as `hidden MS`, with its rows if it has any.
# A frame after they are shown again has one time more, the first: the sync that found the
# bars running again, from which their timers count.
#
# With `now=MS` the scene's `Date.now()` answers MS (printed as `now MS`): what draws by the
# clock (WavyLine's wave starts at `Date.now() / 400`) is then the same in every window. The
# global object of a QML engine cannot be written, so a context property named `Date` stands
# before it, with `now()` alone: a scene that makes a `Date` cannot be taken this way.
#
# Needs OpenGL (wayland under a headless sway) and QSG_RENDER_LOOP=basic, so that the grab
# and the clock readings are on one thread.
import os
import sys
import time

from PySide6.QtCore import QObject, Qt, QTimer, QUrl, Slot
from PySide6.QtGui import QGuiApplication, QImage
from PySide6.QtQuick import QQuickView

DURATION = 1760
src, args = sys.argv[1], sys.argv[2:]
named = dict(a.split("=") for a in args if "=" in a)
hide, show, now = (int(named[k]) if k in named else None for k in ("hide", "show", "now"))
grabs = [int(a) for a in args if "=" not in a]
# The count of syncs when the children were hidden, and when they were shown again.
hidden_at = shown_at = None
app = QGuiApplication(sys.argv)
v = QQuickView()
sync, renders = [], []
v.beforeSynchronizing.connect(lambda: sync.append(time.monotonic_ns()), Qt.DirectConnection)
v.afterSynchronizing.connect(lambda: sync.append(time.monotonic_ns()), Qt.DirectConnection)
v.beforeRendering.connect(lambda: renders.append(time.monotonic_ns()), Qt.DirectConnection)



class Held(QObject):
    @Slot(result=float)
    def now(self):
        return float(now)


if now is not None:
    held = Held()
    v.rootContext().setContextProperty("Date", held)
v.setSource(QUrl.fromLocalFile(src))
if v.status() != QQuickView.Status.Ready:
    sys.exit(f"{src}: {v.errors()}")
v.setMinimumSize(v.size())
v.setMaximumSize(v.size())


def timer(started):
    """(the node's time at the last render, the render that last restarted it)"""
    start, restart, t = started, None, 0
    for r in renders:
        if r < started:
            continue
        t = (r - start) // 1_000_000
        if t >= DURATION:
            t, start, restart = 0, r, r
    return t, restart


def rows(img):
    w, h = img.width(), img.height()
    bg = img.pixel(0, 0)
    out, prev, first = [], None, 0
    for y in range(h + 1):
        line = None
        if y < h:
            px = [img.pixel(x, y) & 0xFFFFFF for x in range(w)]
            if any(p != bg & 0xFFFFFF for p in px):
                runs, s = [], 0
                for x in range(1, w + 1):
                    if x == w or px[x] != px[s]:
                        runs.append(f"{s}-{x - 1}:{px[s]:06x}")
                        s = x
                line = " ".join(runs)
        if line != prev:
            if prev is not None:
                out.append(f"{first}-{y - 1}: {prev}")
            prev, first = line, y
    return out


def grab():
    img = v.grabWindow().convertToFormat(QImage.Format.Format_RGB32)
    if hidden_at is not None and shown_at is None:
        print(f"hidden {(renders[-1] - sync[1]) / 1e6:.3f}")
        print("\n".join(rows(img)))
        return
    # The sync that started the timers: the first of all, or the first after the show.
    first = 0 if shown_at is None else shown_at
    (t, restart), (t2, _) = timer(sync[first]), timer(sync[first + 1])
    ms = [(r - sync[1]) / 1e6 for r in ([restart] if restart else []) + [renders[-1]]]
    if shown_at is not None:
        ms.insert(0, (sync[first + 1] - sync[1]) / 1e6)
    if t != t2 or int(ms[-1] - (ms[-2] if len(ms) > 1 else 0)) != t:
        print(f"# left out: {t} or {t2} ms")
        return
    print("frame", " ".join(f"{m:.3f}" for m in ms))
    print("\n".join(rows(img)))


def set_visible(on):
    global hidden_at, shown_at
    for item in v.rootObject().childItems():
        item.setVisible(on)
    if on:
        shown_at = len(sync)
    else:
        hidden_at = len(sync)


def swapped():
    if len(renders) != 1:
        return
    print(f"size {v.width()} {v.height()}")
    if now is not None:
        print(f"now {now}")
    for ms in grabs:
        QTimer.singleShot(ms, grab)
    if hide is not None:
        QTimer.singleShot(hide, lambda: set_visible(False))
        QTimer.singleShot(show, lambda: set_visible(True))
    # Not app.quit(): tearing the window down from inside a handler crashes.
    QTimer.singleShot(max(grabs) + 300, lambda: (sys.stdout.flush(), os._exit(0)))


v.frameSwapped.connect(swapped)
v.show()
app.exec()

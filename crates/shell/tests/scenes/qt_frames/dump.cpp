// dump SCENE.qml: what Qt prints for a scene stepped frame by frame on a 16 ms animation clock.
// The scene's root has `property int frames`, `property string log` and `function sample(frame)`;
// with `property real mouseX` and `mouseY` the pointer is put there (window coordinates) before the first
// frame. Each frame: the clock advances 16 ms, `sample(i)` runs, then the window is given the
// update request of a frame (the basic render loop: frame-synchronous hover, polish, sync,
// render), so what a frame's hover or polish changes is in the log of the next frame. A scene
// with `function shown(frame)` has it called after the frame: what is on the screen.
// A scene with `property int hoverInterval: N` runs in wall time: N is Qt's interval of
// frame-synchronous hover in ms, and `function wait(frame)` gives the wall time from the start
// of a frame to the start of the next, with the event loop running (the delay timer of the
// hover fires in it). Other scenes are stepped as fast as they go, with that interval at 0.
// Build and run in the testenv image (Qt 6.12): see dump.sh.
#include <QAnimationDriver>
#include <QElapsedTimer>
#include <QEventLoop>
#include <QFile>
#include <QGuiApplication>
#include <QMouseEvent>
#include <QQuickItem>
#include <QQuickView>
#include <QRegularExpression>
#include <QTimer>
#include <QVariantAnimation>
#include <cstdio>

class Clock : public QAnimationDriver {
public:
    qint64 t = 0, started = 0;
    void start() override { started = t; QAnimationDriver::start(); }
    void advance() override { t += 16; advanceAnimation(); }
    qint64 elapsed() const override { return t - started; }
};

// One frame of the render loop: QSGGuiThreadRenderLoop::renderWindow, which a grabWindow() is
// not (it delivers no frame-synchronous hover).
static void frame(QQuickView &v)
{
    v.update();
    QEvent request(QEvent::UpdateRequest);
    QCoreApplication::sendEvent(&v, &request);
}

// The event loop runs until `clock` shows `until` ms: the wall time of its frames is what a
// scene with `hoverInterval` is about, and Qt's delay timer of the hover fires in this loop.
static void runUntil(const QElapsedTimer &clock, qint64 until)
{
    qint64 left = until - clock.elapsed();
    if (left <= 0)
        return;
    QEventLoop loop;
    QTimer::singleShot(left, Qt::PreciseTimer, &loop, &QEventLoop::quit);
    loop.exec();
}

int main(int argc, char **argv)
{
    QFile scene(QString::fromLocal8Bit(argv[1]));
    if (!scene.open(QIODevice::ReadOnly))
        return 1;
    auto interval = QRegularExpression("property int hoverInterval: (-?[0-9]+)")
        .match(QString::fromUtf8(scene.readAll()));
    qputenv("QT_QPA_PLATFORM", "offscreen");
    qputenv("QSG_RENDER_LOOP", "basic");
    qputenv("QT_QUICK_CONTROLS_STYLE", "Basic");
    qputenv("QT_FORCE_STDERR_LOGGING", "1");
    // Qt 6.12 delivers frame-synchronous hover at most once in 100 ms of wall time; 0 is once
    // per frame, which a scene stepped faster than real time can show.
    qputenv("QT_QUICK_FRAME_SYNCHRONOUS_HOVER_INTERVAL",
            interval.hasMatch() ? interval.captured(1).toLatin1() : QByteArray("0"));
    // Frames come from this file only, not from the platform window's update timer.
    qputenv("QT_QPA_UPDATE_IDLE_TIME", "100000000");
    QGuiApplication app(argc, argv);
    Clock clock;
    clock.install();
    // The driver runs from the start, as it does in a shell with anything animated: the step
    // that starts a stopped one is not 16 ms in this file (the wall clock gets into it).
    QVariantAnimation running;
    running.setDuration(1000);
    running.setLoopCount(-1);
    running.start();
    QQuickView v;
    v.setSource(QUrl::fromLocalFile(QString::fromLocal8Bit(argv[1])));
    for (const auto &e : v.errors())
        fprintf(stderr, "LOADERR %s\n", qPrintable(e.toString()));
    v.show();
    app.processEvents();
    QQuickItem *root = v.rootObject();
    frame(v);
    QElapsedTimer wall;
    wall.start();
    // The first frame comes after the interval: the one that began with the window ends in a
    // delivery by the delay timer (the offscreen window is entered at its corner), and
    // another one passes after it.
    if (interval.hasMatch())
        runUntil(wall, 300);
    QVariant mouseX = root->property("mouseX");
    if (mouseX.isValid()) {
        QPointF p(mouseX.toReal(), root->property("mouseY").toReal());
        QMouseEvent ev(QEvent::MouseMove, p, v.mapToGlobal(p), Qt::NoButton, Qt::NoButton, Qt::NoModifier);
        QCoreApplication::sendEvent(&v, &ev);
    }
    int frames = root->property("frames").toInt();
    qint64 next = wall.elapsed();
    for (int i = 1; i <= frames; i++) {
        if (interval.hasMatch()) {
            runUntil(wall, next);
            QVariant ms;
            QMetaObject::invokeMethod(root, "wait", Q_RETURN_ARG(QVariant, ms), Q_ARG(QVariant, i));
            next += ms.toLongLong();
        }
        clock.advance();
        app.processEvents();
        QMetaObject::invokeMethod(root, "sample", Q_ARG(QVariant, i));
        frame(v);
        if (root->metaObject()->indexOfMethod("shown(QVariant)") >= 0)
            QMetaObject::invokeMethod(root, "shown", Q_ARG(QVariant, i));
        // An animation started in this frame registers through two queued calls, one posted
        // by the other: both run before the next step, as they do in an event loop.
        app.processEvents();
        app.processEvents();
    }
    fputs(qPrintable(root->property("log").toString()), stdout);
    return 0;
}

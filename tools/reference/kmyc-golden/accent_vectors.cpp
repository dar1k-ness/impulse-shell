// Named reference vectors for the unit tests in crates/colorgen/src/kde/accent.rs, printed by
// the real Qt 6 (QColor, QColorSpace/QColorTransform), KConfig 6 (KConfigGroup QColor entries)
// and KGuiAddons 6 (KColorUtils), plus Plasma's colour helpers from plasma-workspace 6.8
// kcms/colors/colorsapplicator.{h,cpp}, verbatim (the same copies as qcolor.cpp).
// Built and run by accent_vectors.sh inside the kmyc-golden image.
#include <KColorUtils>
#include <KConfig>
#include <KConfigGroup>
#include <QColor>
#include <QColorSpace>
#include <QColorTransform>
#include <QGuiApplication>
#include <QTemporaryDir>
#include <QtMath>
#include <cstdio>

float lerp(float a, float b, float f) { return (a * (1.0 - f)) + (b * f); }
qreal cubeRootOf(qreal num) { return qPow(num, 1.0 / 3.0); }
qreal cubed(qreal num) { return num * num * num; }
struct LAB { qreal L = 0; qreal a = 0; qreal b = 0; };
LAB linearSRGBToOKLab(const QColor &c)
{
    const auto l = 0.4122214708 * c.redF() + 0.5363325363 * c.greenF() + 0.0514459929 * c.blueF();
    const auto m = 0.2119034982 * c.redF() + 0.6806995451 * c.greenF() + 0.1073969566 * c.blueF();
    const auto s = 0.0883024619 * c.redF() + 0.2817188376 * c.greenF() + 0.6299787005 * c.blueF();
    const auto l_ = cubeRootOf(l);
    const auto m_ = cubeRootOf(m);
    const auto s_ = cubeRootOf(s);
    return LAB{.L = 0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_,
               .a = 1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_,
               .b = 0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_};
}
QColor OKLabToLinearSRGB(LAB lab)
{
    const auto l_ = lab.L + 0.3963377774 * lab.a + 0.2158037573 * lab.b;
    const auto m_ = lab.L - 0.1055613458 * lab.a - 0.0638541728 * lab.b;
    const auto s_ = lab.L - 0.0894841775 * lab.a - 1.2914855480 * lab.b;
    const auto l = cubed(l_);
    const auto m = cubed(m_);
    const auto s = cubed(s_);
    const auto r = +4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s;
    const auto g = -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s;
    const auto b = -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s;
    return QColor::fromRgbF(r, g, b).toRgb();
}
QColor tintColor(const QColor &base, const QColor &with, qreal factor)
{
    static auto toLinearSRGB = QColorSpace(QColorSpace::SRgb).transformationToColorSpace(QColorSpace::SRgbLinear);
    static auto fromLinearSRGB = QColorSpace(QColorSpace::SRgbLinear).transformationToColorSpace(QColorSpace::SRgb);
    auto baseLAB = linearSRGBToOKLab(toLinearSRGB.map(base));
    const auto withLAB = linearSRGBToOKLab(toLinearSRGB.map(with));
    baseLAB.a = lerp(baseLAB.a, withLAB.a, factor);
    baseLAB.b = lerp(baseLAB.b, withLAB.b, factor);
    return fromLinearSRGB.map(OKLabToLinearSRGB(baseLAB));
}
inline QColor alphaBlend(const QColor &foreground, const QColor &background)
{
    const auto foregroundAlpha = foreground.alphaF();
    const auto inverseForegroundAlpha = 1.0 - foregroundAlpha;
    const auto backgroundAlpha = background.alphaF();
    if (foregroundAlpha == 0.0) {
        return background;
    }
    if (backgroundAlpha == 1.0) {
        return QColor::fromRgb((foregroundAlpha * foreground.red()) + (inverseForegroundAlpha * background.red()),
                               (foregroundAlpha * foreground.green()) + (inverseForegroundAlpha * background.green()),
                               (foregroundAlpha * foreground.blue()) + (inverseForegroundAlpha * background.blue()),
                               0xff);
    } else {
        const auto inverseBackgroundAlpha = (backgroundAlpha * inverseForegroundAlpha);
        const auto finalAlpha = foregroundAlpha + inverseBackgroundAlpha;
        return QColor::fromRgb((foregroundAlpha * foreground.red()) + (inverseBackgroundAlpha * background.red()),
                               (foregroundAlpha * foreground.green()) + (inverseBackgroundAlpha * background.green()),
                               (foregroundAlpha * foreground.blue()) + (inverseBackgroundAlpha * background.blue()),
                               finalAlpha);
    }
}
inline QColor accentBackground(const QColor &accent, const QColor &background)
{
    auto c = accent;
    c.setAlphaF(0.7);
    return alphaBlend(c, background);
}
inline QColor accentForeground(const QColor &accent, const bool &isActive)
{
    auto c = QColor(Qt::white);
    if (KColorUtils::contrastRatio(accent, QColor(Qt::white)) < 4.5) {
        c = QColor(Qt::black);
    } else {
        c = QColor(Qt::white);
    }
    if (isActive) {
        c.setAlphaF(1.0);
    } else {
        c.setAlphaF(0.6);
    }
    return alphaBlend(c, accent);
}
QColor readableLink(const QColor &accent, const QColor &background)
{
    auto readableAccent = accent;
    if (KColorUtils::contrastRatio(background, readableAccent) < 5.0) {
        if (KColorUtils::luma(readableAccent) > KColorUtils::luma(background)) {
            readableAccent = KColorUtils::lighten(readableAccent);
        } else {
            readableAccent = KColorUtils::darken(readableAccent);
        }
    }
    return readableAccent;
}

// How a colour is stored: `rgb r16 g16 b16 a16`, `ext rF gF bF aF` (the qfloat16 values) or
// `invalid`.
static void storage(const char *name, const QColor &c)
{
    printf("%s =", name);
    if (!c.isValid()) {
        printf(" invalid\n");
    } else if (c.spec() == QColor::ExtendedRgb) {
        printf(" ext %.9g %.9g %.9g %.9g\n", c.redF(), c.greenF(), c.blueF(), c.alphaF());
    } else {
        const QRgba64 v = c.rgba64();
        printf(" rgb %u %u %u %u\n", v.red(), v.green(), v.blue(), v.alpha());
    }
}
// What KConfig writes for it (`r,g,b[,a]`).
static void put(const char *name, const QColor &c)
{
    printf("%s =", name);
    if (!c.isValid())
        printf(" invalid");
    else if (c.alpha() != 255)
        printf(" %d,%d,%d,%d", c.red(), c.green(), c.blue(), c.alpha());
    else
        printf(" %d,%d,%d", c.red(), c.green(), c.blue());
    printf("\n");
}
static void num(const char *name, double v) { printf("%s = %.17g\n", name, v); }

int main(int argc, char **argv)
{
    QGuiApplication app(argc, argv);
    char n[256];

    // QColor storage and roundings.
    printf("### qcolor\n");
    storage("fromRgbF(0.5,0.25,0.333333343,0.7)", QColor::fromRgbF(0.5f, 0.25f, 1.0f / 3.0f, 0.7f));
    storage("fromRgbF(0.00001,0.99999,0.5000076,1)", QColor::fromRgbF(0.00001f, 0.99999f, 0.5000076f, 1.0f));
    storage("fromRgbF(1.2,-0.1,0.333333343,1)", QColor::fromRgbF(1.2f, -0.1f, 1.0f / 3.0f, 1.0f));
    storage("fromRgbF(0.5,1.5,0.25,1.2)", QColor::fromRgbF(0.5f, 1.5f, 0.25f, 1.2f));
    storage("fromRgbF(1.2,-0.1,0.333333343,1).toRgb()", QColor::fromRgbF(1.2f, -0.1f, 1.0f / 3.0f, 1.0f).toRgb());
    put("fromRgbF(1.2,-0.1,0.333333343,1).rgba8", QColor::fromRgbF(1.2f, -0.1f, 1.0f / 3.0f, 1.0f));
    put("fromRgbF(0.5,1.5,0.25,0.6).rgba8", QColor::fromRgbF(0.5f, 1.5f, 0.25f, 0.6f));
    {
        QColor c(61, 174, 233);
        c.setAlphaF(0.7f);
        storage("QColor(61,174,233).setAlphaF(0.7)", c);
        put("QColor(61,174,233).setAlphaF(0.7).rgba8", c);
        QColor e = QColor::fromRgbF(1.2f, -0.1f, 0.5f, 1.0f);
        e.setAlphaF(0.7f);
        storage("fromRgbF(1.2,-0.1,0.5,1).setAlphaF(0.7)", e);
    }
    for (unsigned v : {128u, 129u, 383u, 384u, 32767u, 32768u, 32895u, 65407u, 65408u, 65535u}) {
        snprintf(n, sizeof n, "QRgba64(%u).red", v);
        printf("%s = %d\n", n, QColor(QRgba64::fromRgba64(v, 0, 0, 0xffff)).red());
    }

    // KConfig QColor entries (kconfiggroupgui.cpp), read with readEntry(key, QColor()).
    printf("### kconfig\n");
    QTemporaryDir dir;
    const QString path = dir.filePath("rc");
    {
        FILE *f = fopen(path.toLocal8Bit().constData(), "w");
        fputs("[G]\n"
              "a=61,174,233\n"
              "b=255,0,0,128\n"
              "c= 10 , 20 , 30 \n"
              "d=1,2\n"
              "e=256,0,0\n"
              "f=1,2,3,4,5\n"
              "g=#3daee9\n"
              "h=#abc\n"
              "i=#803daee9\n"
              "j=#fff000888\n"
              "k=#ffff00008000\n"
              "l=aliceblue\n"
              "m=Alice Blue\n"
              "n=transparent\n"
              "o=invalid\n"
              "p=\n"
              "q=#12345\n"
              "r=nosuchcolour\n"
              "s=-1,0,0\n"
              "t=a,b,c\n",
              f);
        fclose(f);
        KConfig cfg(path, KConfig::SimpleConfig);
        KConfigGroup g(&cfg, QStringLiteral("G"));
        for (const char *k : {"a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t"}) {
            snprintf(n, sizeof n, "read(%s)", g.readEntry(k, QString()).toUtf8().constData());
            storage(n, g.readEntry(k, QColor()));
        }
        storage("read(<absent>)", g.readEntry("absent", QColor()));
    }
    {
        KConfig cfg(path, KConfig::SimpleConfig);
        KConfigGroup g(&cfg, QStringLiteral("W"));
        QColor half(61, 174, 233);
        half.setAlphaF(0.7f);
        g.writeEntry("opaque", QColor(61, 174, 233));
        g.writeEntry("alpha", half);
        g.writeEntry("ext", QColor::fromRgbF(1.2f, -0.1f, 0.5f, 0.6f));
        g.writeEntry("invalid", QColor());
        for (const char *k : {"opaque", "alpha", "ext", "invalid"}) {
            snprintf(n, sizeof n, "write(%s)", k);
            printf("%s = %s\n", n, g.readEntry(k, QString()).toUtf8().constData());
        }
    }

    // QColorTransform sRGB <-> linear sRGB, as tintColor builds them.
    printf("### transform\n");
    const auto toLin = QColorSpace(QColorSpace::SRgb).transformationToColorSpace(QColorSpace::SRgbLinear);
    const auto fromLin = QColorSpace(QColorSpace::SRgbLinear).transformationToColorSpace(QColorSpace::SRgb);
    const int rgbs[][3] = {{0, 0, 0}, {255, 255, 255}, {1, 2, 3}, {10, 20, 30}, {61, 174, 233}, {28, 32, 36}, {200, 30, 90}};
    for (const auto &c : rgbs) {
        snprintf(n, sizeof n, "toLinear(%d,%d,%d)", c[0], c[1], c[2]);
        storage(n, toLin.map(QColor(c[0], c[1], c[2])));
        snprintf(n, sizeof n, "fromLinear(%d,%d,%d)", c[0], c[1], c[2]);
        storage(n, fromLin.map(QColor(c[0], c[1], c[2])));
    }
    storage("toLinear(fromRgbF(1.2,-0.1,0.5,0.6))", toLin.map(QColor::fromRgbF(1.2f, -0.1f, 0.5f, 0.6f)));
    storage("fromLinear(fromRgbF(1.2,-0.1,0.5,0.6))", fromLin.map(QColor::fromRgbF(1.2f, -0.1f, 0.5f, 0.6f)));
    {
        QColor a(61, 174, 233);
        a.setAlphaF(0.7f);
        storage("toLinear(61,174,233,a0.7)", toLin.map(a));
    }

    // KColorUtils (HCY).
    printf("### kcolorutils\n");
    const int hcys[][3] = {{0, 0, 0}, {255, 255, 255}, {128, 128, 128}, {255, 0, 0}, {0, 255, 0}, {0, 0, 255}, {255, 255, 0}, {0, 255, 255}, {255, 0, 255}, {61, 174, 233}, {200, 30, 90}, {28, 32, 36}, {247, 249, 255}};
    for (const auto &c : hcys) {
        const QColor q(c[0], c[1], c[2]);
        snprintf(n, sizeof n, "hue(%d,%d,%d)", c[0], c[1], c[2]);
        num(n, KColorUtils::hue(q));
        snprintf(n, sizeof n, "chroma(%d,%d,%d)", c[0], c[1], c[2]);
        num(n, KColorUtils::chroma(q));
        snprintf(n, sizeof n, "luma(%d,%d,%d)", c[0], c[1], c[2]);
        num(n, KColorUtils::luma(q));
        snprintf(n, sizeof n, "hcyColor(hcy(%d,%d,%d))", c[0], c[1], c[2]);
        storage(n, KColorUtils::hcyColor(KColorUtils::hue(q), KColorUtils::chroma(q), KColorUtils::luma(q)));
        snprintf(n, sizeof n, "lighten(%d,%d,%d)", c[0], c[1], c[2]);
        put(n, KColorUtils::lighten(q));
        snprintf(n, sizeof n, "darken(%d,%d,%d)", c[0], c[1], c[2]);
        put(n, KColorUtils::darken(q));
    }
    const double triples[][4] = {{0.1, 0.5, 0.3, 1}, {0.9, 1.2, 0.5, 1}, {-0.2, 0.4, 0.6, 1}, {1.5, 0.3, 0.2, 1}, {0.5, 1.0, 1.0, 1}, {0.6, 0.8, 0.05, 0.5}, {0.25, 0.7, 0.9, 1}, {0.75, 0.7, 0.1, 1}};
    for (const auto &t : triples) {
        snprintf(n, sizeof n, "hcyColor(%g,%g,%g,%g)", t[0], t[1], t[2], t[3]);
        storage(n, KColorUtils::hcyColor(t[0], t[1], t[2], t[3]));
    }
    {
        QColor a(61, 174, 233);
        a.setAlphaF(0.5f);
        put("lighten(61,174,233,a0.5)", KColorUtils::lighten(a));
    }
    const int pairs[][6] = {{0, 0, 0, 255, 255, 255}, {61, 174, 233, 28, 32, 36}, {61, 174, 233, 247, 249, 255}, {200, 30, 90, 255, 255, 255}};
    for (const auto &p : pairs) {
        snprintf(n, sizeof n, "contrastRatio(%d,%d,%d;%d,%d,%d)", p[0], p[1], p[2], p[3], p[4], p[5]);
        num(n, KColorUtils::contrastRatio(QColor(p[0], p[1], p[2]), QColor(p[3], p[4], p[5])));
    }

    // colorsapplicator helpers.
    printf("### colorsapplicator\n");
    struct Tint { int base[3], with[3]; double f; };
    const Tint tints[] = {
        {{28, 32, 36}, {61, 174, 233}, 0.15}, {{247, 249, 255}, {61, 174, 233}, 0.15},
        {{28, 32, 36}, {200, 30, 90}, 0.5},   {{193, 199, 206}, {200, 30, 90}, 1.0},
        {{63, 69, 75}, {146, 110, 228}, 0.0}, {{255, 255, 255}, {0, 0, 255}, 1.0},
        {{0, 0, 0}, {255, 0, 0}, 0.3},        {{1, 2, 3}, {255, 255, 0}, 0.15},
    };
    for (const auto &t : tints) {
        snprintf(n, sizeof n, "tintColor(%d,%d,%d;%d,%d,%d;%g)", t.base[0], t.base[1], t.base[2], t.with[0], t.with[1], t.with[2], t.f);
        storage(n, tintColor(QColor(t.base[0], t.base[1], t.base[2]), QColor(t.with[0], t.with[1], t.with[2]), t.f));
    }
    const int links[][6] = {
        {61, 174, 233, 28, 32, 36},  {61, 174, 233, 247, 249, 255}, {20, 40, 90, 28, 32, 36},
        {250, 240, 120, 247, 249, 255}, {200, 30, 90, 255, 255, 255}, {200, 30, 90, 0, 0, 0},
        {143, 201, 252, 28, 32, 36},
    };
    for (const auto &l : links) {
        snprintf(n, sizeof n, "readableLink(%d,%d,%d;%d,%d,%d)", l[0], l[1], l[2], l[3], l[4], l[5]);
        put(n, readableLink(QColor(l[0], l[1], l[2]), QColor(l[3], l[4], l[5])));
        snprintf(n, sizeof n, "accentBackground(%d,%d,%d;%d,%d,%d)", l[0], l[1], l[2], l[3], l[4], l[5]);
        const QColor abg = accentBackground(QColor(l[0], l[1], l[2]), QColor(l[3], l[4], l[5]));
        put(n, abg);
        snprintf(n, sizeof n, "accentForeground(accentBackground(%d,%d,%d;%d,%d,%d),true)", l[0], l[1], l[2], l[3], l[4], l[5]);
        put(n, accentForeground(abg, true));
        snprintf(n, sizeof n, "accentForeground(%d,%d,%d,false)", l[3], l[4], l[5]);
        put(n, accentForeground(QColor(l[3], l[4], l[5]), false));
    }
    {
        QColor bg(10, 20, 30);
        bg.setAlphaF(0.5f);
        put("accentBackground(61,174,233;10,20,30,a0.5)", accentBackground(QColor(61, 174, 233), bg));
        QColor clear(61, 174, 233);
        clear.setAlphaF(0.0f);
        put("alphaBlend(61,174,233,a0;10,20,30)", alphaBlend(clear, QColor(10, 20, 30)));
    }
    return 0;
}

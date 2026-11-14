// Reference values for crates/colorgen/src/kde/accent.rs: Plasma's accent colour maths
// (colorsapplicator.cpp/.h of plasma-workspace 6.8, verbatim) on the real Qt and KGuiAddons.
// Built and run by qcolor.sh inside the kmyc-golden image; one line per random case:
//   base accent bg factor | tint lighten darken link accentbg fgactive fginactive | luma contrast
#include <KColorUtils>
#include <QColor>
#include <QColorSpace>
#include <QtMath>
#include <cstdio>
#include <random>

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

static void put(const QColor &c)
{
    if (!c.isValid()) {
        printf(" invalid");
        return;
    }
    printf(" %d,%d,%d", c.red(), c.green(), c.blue());
    if (c.alpha() != 255)
        printf(",%d", c.alpha());
}

int main()
{
    std::mt19937 rng(20261104);
    auto byte = [&]() { return int(rng() % 256); };
    // A few fixed edge colours, then random ones.
    const int edges[][3] = {{0, 0, 0}, {255, 255, 255}, {128, 128, 128}, {255, 0, 0}, {0, 255, 0}, {0, 0, 255}, {1, 1, 1}, {254, 254, 254}};
    for (int i = 0; i < 4000; ++i) {
        QColor base, accent, bg;
        if (i < 512) {
            const auto &e1 = edges[i % 8], &e2 = edges[(i / 8) % 8], &e3 = edges[(i / 64) % 8];
            base = QColor(e1[0], e1[1], e1[2]);
            accent = QColor(e2[0], e2[1], e2[2]);
            bg = QColor(e3[0], e3[1], e3[2]);
        } else {
            base = QColor(byte(), byte(), byte());
            accent = QColor(byte(), byte(), byte());
            bg = QColor(byte(), byte(), byte());
        }
        const double factor = (i % 3 == 0) ? 0.15 : (rng() % 1001) / 1000.0;
        put(base), put(accent), put(bg);
        printf(" %.17g |", factor);
        put(tintColor(base, accent, factor));
        put(KColorUtils::lighten(accent));
        put(KColorUtils::darken(accent));
        put(readableLink(accent, bg));
        const QColor abg = accentBackground(accent, bg);
        put(abg);
        put(accentForeground(abg, true));
        put(accentForeground(base, false));
        printf(" | %.17g %.17g\n", KColorUtils::luma(accent), KColorUtils::contrastRatio(bg, accent));
    }
}

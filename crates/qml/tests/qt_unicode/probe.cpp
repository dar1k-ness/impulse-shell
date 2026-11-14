// What Qt 6.12 holds of Unicode for the code points of points.txt, and what the ICU beside it
// (the same Unicode version) normalizes them to. dump.sh builds and runs this.
#include <QtCore/QChar>
#include <QtCore/QFile>
#include <QtCore/QString>
#include <QtCore/QUrl>
#include <unicode/normalizer2.h>
#include <unicode/uchar.h>
#include <unicode/unistr.h>
#include <cstdio>

static QByteArray hex(const QString &s)
{
    QByteArray out;
    for (char32_t c : s.toUcs4()) {
        if (!out.isEmpty())
            out += ' ';
        out += QByteArray::number(uint(c), 16).toUpper();
    }
    return out;
}

static QString str(char32_t c)
{
    QString s;
    for (QChar ch : QChar::fromUcs4(c))
        s += ch;
    return s;
}

static QString by_icu(const icu::Normalizer2 *n, const QString &s)
{
    UErrorCode e = U_ZERO_ERROR;
    const icu::UnicodeString r = n->normalize(icu::UnicodeString(s.utf16(), s.size()), e);
    return QString::fromUtf16(r.getBuffer(), r.length());
}

static QByteArray url(const QString &host)
{
    return QUrl(u"http://" + host + u"/").toString().toUtf8();
}

int main(int argc, char **argv)
{
    if (argc != 2)
        return 2;
    const char *version = nullptr;
    switch (QChar::currentUnicodeVersion()) {
    case QChar::Unicode_16_0: version = "16.0"; break;
    case QChar::Unicode_17_0: version = "17.0"; break;
    default: return 3;
    }
    int assigned = 0;
    for (char32_t c = 0; c <= 0x10ffff; ++c)
        assigned += !QChar::isSurrogate(c) && QChar::category(c) != QChar::Other_NotAssigned;
    UVersionInfo v;
    u_getUnicodeVersion(v);
    printf("unicode\t%s\t%d\t%d.%d\n", version, assigned, v[0], v[1]);

    static const char *bidi[] = {"L", "R", "EN", "ES", "ET", "AN", "CS", "B", "S", "WS", "ON", "LRE",
                                 "LRO", "AL", "RLE", "RLO", "PDF", "NSM", "BN", "LRI", "RLI", "FSI", "PDI"};
    UErrorCode e = U_ZERO_ERROR;
    const QString::NormalizationForm forms[] = {QString::NormalizationForm_C, QString::NormalizationForm_D,
                                                QString::NormalizationForm_KC, QString::NormalizationForm_KD};
    const char *names[] = {"NFC", "NFD", "NFKC", "NFKD"};
    const icu::Normalizer2 *icus[] = {icu::Normalizer2::getNFCInstance(e), icu::Normalizer2::getNFDInstance(e),
                                      icu::Normalizer2::getNFKCInstance(e), icu::Normalizer2::getNFKDInstance(e)};
    if (U_FAILURE(e))
        return 4;

    QFile points(QString::fromLocal8Bit(argv[1]));
    if (!points.open(QIODevice::ReadOnly))
        return 5;
    while (!points.atEnd()) {
        const QByteArray line = points.readLine().trimmed();
        if (line.isEmpty() || line.startsWith('#'))
            continue;
        const QList<QByteArray> f = line.split('\t');
        const char32_t c = f[1].toUInt(nullptr, 16);
        const QString s = str(c);
        // kind, code point, block, assigned, Bidi_Class, and the url of three hosts with it.
        printf("%s\t%s\t%s\t%d\t%s\t%s\t%s\t%s\n", f[0].constData(), f[1].constData(), f[2].constData(),
               QChar::category(c) != QChar::Other_NotAssigned, bidi[QChar::direction(c)],
               url(s + u".com").constData(), url(u"a" + s + u"b.com").constData(),
               url(u"\u05d0" + s + u"\u05d0.com").constData());
        if (f[0] != "nfc")
            continue;
        // The code point alone, decomposed, and between a letter with a mark below and a mark above.
        const QString inputs[] = {s, by_icu(icus[1], s), u"a\u0323" + s + u"\u0301"};
        for (const QString &in : inputs)
            for (int i = 0; i < 4; ++i)
                printf("normalized\t%s\t%s\t%s\t%s\t%s\n", f[1].constData(), hex(in).constData(), names[i],
                       hex(in.normalized(forms[i])).constData(), hex(by_icu(icus[i], in)).constData());
    }
    return 0;
}

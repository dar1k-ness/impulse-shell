// What Qt holds for the Unicode Bidirectional Algorithm, for every code point but the
// surrogates: its class (QChar::direction) and, the way QTextEngine pairs brackets in rule N0
// (qtextengine.cpp, resolveN0: a code unit of class ON with a mirrored character that is
// Punctuation_Open or Punctuation_Close), the bracket it opens or closes.
//
// Then the script of every code point (QChar::script), which QUnicodeTools::initScripts splits
// a line into items by.
//
// Lines: `class FIRST..LAST NAME` for each run of code points of one class, `open C CLOSING`
// and `close C OPENING` for each bracket, `script FIRST..LAST N` for each run of code points
// of one script (N is QChar::Script), `assigned N` and `unicode N` (QChar::UnicodeVersion).
#include <QChar>
#include <cstdio>

static const char *name(QChar::Direction d)
{
    switch (d) {
    case QChar::DirL: return "L";
    case QChar::DirR: return "R";
    case QChar::DirEN: return "EN";
    case QChar::DirES: return "ES";
    case QChar::DirET: return "ET";
    case QChar::DirAN: return "AN";
    case QChar::DirCS: return "CS";
    case QChar::DirB: return "B";
    case QChar::DirS: return "S";
    case QChar::DirWS: return "WS";
    case QChar::DirON: return "ON";
    case QChar::DirLRE: return "LRE";
    case QChar::DirLRO: return "LRO";
    case QChar::DirAL: return "AL";
    case QChar::DirRLE: return "RLE";
    case QChar::DirRLO: return "RLO";
    case QChar::DirPDF: return "PDF";
    case QChar::DirNSM: return "NSM";
    case QChar::DirBN: return "BN";
    case QChar::DirLRI: return "LRI";
    case QChar::DirRLI: return "RLI";
    case QChar::DirFSI: return "FSI";
    case QChar::DirPDI: return "PDI";
    }
    return "?";
}

static bool surrogate(char32_t c) { return c >= 0xD800 && c <= 0xDFFF; }

int main()
{
    char32_t first = 0;
    int assigned = 0;
    for (char32_t c = 0; c <= 0x110000; ++c) {
        if (surrogate(c))
            continue;
        char32_t before = c == 0xE000 ? 0xD7FF : c - 1;
        if (c == 0x110000 || (c > 0 && QChar::direction(c) != QChar::direction(before))) {
            printf("class %04X..%04X %s\n", unsigned(first), unsigned(before), name(QChar::direction(before)));
            first = c;
        }
        if (c < 0x110000 && QChar::category(c) != QChar::Other_NotAssigned)
            ++assigned;
    }
    for (char32_t c = 0; c <= 0xFFFF; ++c) {
        if (surrogate(c) || QChar::direction(c) != QChar::DirON || QChar::mirroredChar(c) == c)
            continue;
        if (QChar::category(c) == QChar::Punctuation_Open)
            printf("open %04X %04X\n", unsigned(c), unsigned(QChar::mirroredChar(c)));
        else if (QChar::category(c) == QChar::Punctuation_Close)
            printf("close %04X %04X\n", unsigned(c), unsigned(QChar::mirroredChar(c)));
    }
    char32_t from = 0;
    for (char32_t c = 1; c <= 0x110000; ++c) {
        if (surrogate(c))
            continue;
        if (c == 0x110000 || QChar::script(c) != QChar::script(from) || c == 0xE000) {
            printf("script %04X..%04X %d\n", unsigned(from), unsigned(c == 0xE000 ? 0xD7FF : c - 1),
                   int(QChar::script(from)));
            from = c;
        }
    }
    printf("assigned %d\n", assigned);
    printf("unicode %d\n", int(QChar::currentUnicodeVersion()));
}

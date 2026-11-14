// What Qt gives HarfBuzz for the Unicode data it shapes with: the six functions of
// hb_qt_get_unicode_funcs() (qharfbuzzng.cpp), which QTextEngine sets on every buffer in
// place of HarfBuzz's own tables. Qt does not export them, so the probe stands in for
// hb_buffer_set_unicode_funcs, lays out one line and keeps the functions Qt hands over.
//
// Lines, for every code point but the surrogates:
//   `category FIRST..LAST N`    a run of one general category (hb_unicode_general_category_t)
//   `combining FIRST..LAST N`   a run of one combining class other than 0
//   `script FIRST..LAST TAG`    a run of one script (the ISO 15924 tag of hb_script_t)
//   `mirror C M`                a character with a mirrored one
//   `decompose AB A B`          a character the function decomposes (B is 0 for a singleton)
//   `first FIRST..LAST`, `second B`  the pairs asked of the compose function: every first and
//                               every second part of a decomposition, Qt's or HarfBuzz's own
//   `compose A B AB`            a pair of them the function composes
//   `hangul DECOMPOSED COMPOSED` the syllables of U+AC00..U+D7A3 and the pairs making them,
//                               left out of the lines above where they are the arithmetic of
//                               the standard (3.12)
//   `harfbuzz FUNCTION DIFFER ASSIGNED`  code points (pairs for compose) where the tables of
//                               this HarfBuzz answer otherwise, and those of them Qt has assigned
//   `unassigned-next N`         code points Qt has not assigned and this HarfBuzz has
#include <QGuiApplication>
#include <QTextLayout>
#include <cstdio>
#include <dlfcn.h>
#include <hb.h>
#include <map>
#include <set>
#include <utility>

static hb_unicode_funcs_t *qt = nullptr;

extern "C" void hb_buffer_set_unicode_funcs(hb_buffer_t *buffer, hb_unicode_funcs_t *funcs)
{
    using Set = void (*)(hb_buffer_t *, hb_unicode_funcs_t *);
    static Set real = Set(dlsym(RTLD_NEXT, "hb_buffer_set_unicode_funcs"));
    if (!qt)
        qt = hb_unicode_funcs_reference(funcs);
    real(buffer, funcs);
}

static bool hangul_decomposes(unsigned ab, unsigned a, unsigned b)
{
    if (ab < 0xAC00 || ab > 0xD7A3)
        return false;
    unsigned s = ab - 0xAC00, t = s % 28;
    return t ? a == ab - t && b == 0x11A7 + t : a == 0x1100 + s / 588 && b == 0x1161 + s % 588 / 28;
}

template <typename F> static void runs(const char *name, F value, bool (*print)(long))
{
    long first = 0, was = -1;
    for (unsigned c = 0; c <= 0x110000; ++c) {
        if (c >= 0xD800 && c <= 0xDFFF)
            continue;
        long v = c == 0x110000 ? -2 : long(value(c));
        if (v == was)
            continue;
        if (was != -1 && print(was)) {
            unsigned last = c == 0xE000 ? 0xD7FF : c - 1;
            if (was > 0xFFFF)
                printf("%s %04lX..%04X %c%c%c%c\n", name, first, last, char(was >> 24),
                       char(was >> 16), char(was >> 8), char(was));
            else
                printf("%s %04lX..%04X %ld\n", name, first, last, was);
        }
        first = c;
        was = v;
    }
}

struct Count {
    const char *name;
    long differ = 0, assigned = 0;
    void hit(bool a) { ++differ; assigned += a; }
};

int main(int argc, char **argv)
{
    qputenv("QT_QPA_PLATFORM", "offscreen");
    QGuiApplication app(argc, argv);
    QTextLayout layout(QStringLiteral("a"), QFont());
    layout.beginLayout();
    layout.createLine();
    layout.endLayout();
    if (!qt) {
        fprintf(stderr, "Qt did not set its Unicode functions on a buffer\n");
        return 1;
    }
    hb_unicode_funcs_t *own = hb_unicode_funcs_get_default();
    auto category = [](hb_unicode_funcs_t *f, unsigned c) { return hb_unicode_general_category(f, c); };
    auto assigned = [&](unsigned c) { return category(qt, c) != HB_UNICODE_GENERAL_CATEGORY_UNASSIGNED; };

    runs("category", [&](unsigned c) { return category(qt, c); }, [](long) { return true; });
    runs("combining", [&](unsigned c) { return hb_unicode_combining_class(qt, c); },
         [](long v) { return v != 0; });
    runs("script", [&](unsigned c) { return hb_script_to_iso15924_tag(hb_unicode_script(qt, c)); },
         [](long) { return true; });

    Count gc{"category"}, cc{"combining"}, sc{"script"}, mi{"mirror"}, de{"decompose"}, co{"compose"};
    std::set<unsigned> firsts, seconds;
    long hangul_decomposed = 0, hangul_composed = 0, next = 0;
    for (unsigned c = 0; c < 0x110000; ++c) {
        if (c >= 0xD800 && c <= 0xDFFF)
            continue;
        bool a = assigned(c);
        if (!a && category(own, c) != HB_UNICODE_GENERAL_CATEGORY_UNASSIGNED)
            ++next;
        if (category(qt, c) != category(own, c))
            gc.hit(a);
        if (hb_unicode_combining_class(qt, c) != hb_unicode_combining_class(own, c))
            cc.hit(a);
        if (hb_unicode_script(qt, c) != hb_unicode_script(own, c))
            sc.hit(a);
        unsigned m = hb_unicode_mirroring(qt, c);
        if (m != c)
            printf("mirror %04X %04X\n", c, m);
        if (m != hb_unicode_mirroring(own, c))
            mi.hit(a);
        hb_codepoint_t qa = c, qb = 0, ha = c, hb = 0;
        bool qd = hb_unicode_decompose(qt, c, &qa, &qb), hd = hb_unicode_decompose(own, c, &ha, &hb);
        if (qd != hd || (qd && (qa != ha || qb != hb)))
            de.hit(a);
        if (qd) {
            if (hangul_decomposes(c, qa, qb))
                ++hangul_decomposed;
            else
                printf("decompose %04X %04X %04X\n", c, qa, qb);
            firsts.insert({c, qa});
            if (qb)
                seconds.insert(qb);
        }
        if (hd) {
            firsts.insert({c, ha});
            if (hb)
                seconds.insert(hb);
        }
    }
    long first = -1, last = -1;
    for (unsigned c : firsts) {
        if (long(c) != last + 1) {
            if (first >= 0)
                printf("first %04lX..%04lX\n", first, last);
            first = c;
        }
        last = c;
    }
    printf("first %04lX..%04lX\n", first, last);
    for (unsigned c : seconds)
        printf("second %04X\n", c);
    for (unsigned a : firsts) {
        for (unsigned b : seconds) {
            hb_codepoint_t q = 0, h = 0;
            bool qr = hb_unicode_compose(qt, a, b, &q), hr = hb_unicode_compose(own, a, b, &h);
            if (qr != hr || (qr && q != h))
                co.hit(assigned(a) && assigned(b));
            if (!qr)
                continue;
            if (hangul_decomposes(q, a, b))
                ++hangul_composed;
            else
                printf("compose %04X %04X %04X\n", a, b, q);
        }
    }
    printf("hangul %ld %ld\n", hangul_decomposed, hangul_composed);
    for (const Count &c : {gc, cc, sc, mi, de, co})
        printf("harfbuzz %s %ld %ld\n", c.name, c.differ, c.assigned);
    printf("unassigned-next %ld\n", next);
    printf("harfbuzz-version %s\n", hb_version_string());
}

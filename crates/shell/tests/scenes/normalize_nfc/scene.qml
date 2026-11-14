// String.prototype.normalize of the six code points QString::normalized of Qt 6.12 does not
// compose (docs/qt-differences.md): each alone, decomposed, and between a letter with a mark
// below and a mark above, in the four forms. `rows()` is one line per string and form, as the
// `normalized` rows of crates/qml/tests/qt_unicode/qt.tsv.
import QtQuick
Item {
    property var points: [0x105C9, 0x105E4, 0x16126, 0x16127, 0x16128, 0x16D6A]

    function hex(s) {
        let out = []
        for (let i = 0; i < s.length; i += out[out.length - 1].length > 4 ? 2 : 1)
            out.push(s.codePointAt(i).toString(16).toUpperCase())
        return out.join(" ")
    }

    function rows() {
        let out = []
        for (const p of points) {
            const c = String.fromCodePoint(p)
            for (const input of [c, c.normalize("NFD"), "a\u0323" + c + "\u0301"]) {
                out.push([hex(c), hex(input), "NFC", hex(input.normalize())].join("\t"))
                for (const form of ["NFD", "NFKC", "NFKD"])
                    out.push([hex(c), hex(input), form, hex(input.normalize(form))].join("\t"))
            }
        }
        return out.join("\n")
    }
}

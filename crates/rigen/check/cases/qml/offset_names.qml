// A zone's short name where ICU has an offset for it (`UTC+11` in fr_FR, fi_FI, nds_DE), in
// translated JS as in the interpreter: six letters that begin with `UTC` get `:00`. The
// expressions of crates/qml/tests/system_zone_names_qt/offset_names.cases, answered in each
// of its environments and compared with Qt 6.12's answers there.
// answers: ../../../../qml/tests/system_zone_names_qt/offset_names.cases
import QtQml
QtObject {
    property string result: [
        Qt.locale("fr_FR").toString(new Date(2024, 0, 10, 9, 5, 7), "t|tt|ttt|tttt"),
        Qt.locale("fi_FI").toString(new Date(2024, 0, 10, 9, 5, 7), "t|tt|ttt|tttt"),
        Qt.locale("nds_DE").toString(new Date(2024, 0, 10, 9, 5, 7), "t|tt|ttt|tttt"),
        Qt.locale("br_FR").toString(new Date(2024, 0, 10, 9, 5, 7), "t|tt|ttt|tttt"),
        Qt.locale("de_DE").toString(new Date(2024, 0, 10, 9, 5, 7), "t|tt|ttt|tttt"),
        Qt.locale("fr_CA").toString(new Date(2024, 6, 10, 9, 5, 7), "t|tt|ttt|tttt"),
        Qt.locale("oc_ES").toString(new Date(2024, 6, 10, 9, 5, 7), "t") + "|" + Qt.locale("eo_001").toString(new Date(2024, 6, 10, 9, 5, 7), "t"),
        new Date(2024, 0, 10, 9, 5, 7).toLocaleTimeString(Qt.locale("fr_FR"), Locale.LongFormat),
        Qt.formatDateTime(new Date(2024, 0, 10, 9, 5, 7), Qt.locale("fi_FI"), "t (t) ttttt"),
        Qt.formatDateTime(new Date(2024, 0, 10, 9, 5, 7), "t|tt|ttt|tttt"),
        Qt.locale().toString(new Date(2024, 0, 10, 9, 5, 7), "t|tt|ttt|tttt"),
        new Date(2024, 6, 10, 9, 5, 7).toLocaleString(Qt.locale(), "t") + "|" + new Date(2024, 0, 10, 9, 5, 7).toLocaleTimeString(),
    ].map(v => typeof v + ":" + v).join("\n")
}

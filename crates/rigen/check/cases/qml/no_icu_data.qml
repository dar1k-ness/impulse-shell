// A zone's names in a locale ICU has no data for (`iu_CA`, `nds_DE`), in translated JS as in
// the interpreter: long and short, they are in ICU's default locale, the one the environment
// names for messages. The expressions of
// crates/qml/tests/system_zone_names_qt/no_icu_data.cases, answered in each of its
// environments and compared with Qt 6.12's answers there.
// answers: ../../../../qml/tests/system_zone_names_qt/no_icu_data.cases
import QtQml
QtObject {
    property string result: [
        Qt.locale("iu_CA").toString(new Date(2024, 0, 10, 9, 5, 7), "t|tt|ttt|tttt"),
        Qt.locale("kxv_IN").toString(new Date(2024, 0, 10, 9, 5, 7), "t|tt|ttt|tttt"),
        Qt.locale("nds_DE").toString(new Date(2024, 0, 10, 9, 5, 7), "t|tt|ttt|tttt"),
        Qt.locale("la_VA").toString(new Date(2024, 0, 10, 9, 5, 7), "t|tt|ttt|tttt"),
        Qt.locale("bm_ML").toString(new Date(2024, 0, 10, 9, 5, 7), "t|tt|ttt|tttt"),
        Qt.locale("C").toString(new Date(2024, 0, 10, 9, 5, 7), "t|tt|ttt|tttt"),
        Qt.locale("iu_Latn_CA").toString(new Date(2024, 0, 10, 9, 5, 7), "t|tt|ttt|tttt"),
        Qt.locale("kaa_Latn_UZ").toString(new Date(2024, 0, 10, 9, 5, 7), "t|tt|ttt|tttt"),
        new Date(2024, 0, 10, 9, 5, 7).toLocaleString(Qt.locale("nds_DE"), Locale.LongFormat),
        Qt.formatDateTime(new Date(2024, 0, 10, 9, 5, 7), Qt.locale("iu_CA"), Locale.LongFormat),
        Qt.locale().name + "/" + Qt.locale("iu_CA").name + "/" + Qt.locale("kaa_Latn_UZ").name,
    ].map(v => typeof v + ":" + v).join("\n")
}

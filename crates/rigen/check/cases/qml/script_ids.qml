// A zone's long name where the locale is C and the environment's locale names a script
// (`LANG=sr_Latn_RS`, `uz_Cyrl_UZ`, `ru_Latn_RU`), in translated JS as in the interpreter:
// the name is in ICU's default locale, which keeps the script. The expressions of
// crates/qml/tests/system_zone_names_qt/script_ids.cases, answered in each of its
// environments and compared with Qt 6.12's answers there.
// answers: ../../../../qml/tests/system_zone_names_qt/script_ids.cases
import QtQml
QtObject {
    property string result: [
        Qt.formatDateTime(new Date(2024, 0, 10, 9, 5, 7), "tttt"),
        Qt.formatDateTime(new Date(2024, 6, 10, 9, 5, 7), "tttt"),
        Qt.formatTime(new Date(2024, 6, 10, 9, 5, 7), "tttt"),
        Qt.locale("C").toString(new Date(2024, 0, 10, 9, 5, 7), "tttt"),
        new Date(2024, 0, 10, 9, 5, 7).toLocaleString(Qt.locale("C"), "tttt"),
        new Date(2024, 6, 10, 9, 5, 7).toLocaleTimeString(Qt.locale("C"), "tttt"),
        new Date(2024, 0, 10, 9, 5, 7).toLocaleTimeString(Qt.locale("xx_YY"), "tttt"),
        new Date(2024, 0, 10, 9, 5, 7).toLocaleString(Qt.locale(), "tttt"),
        new Date(2024, 0, 10, 9, 5, 7).toLocaleString(Qt.locale("de_DE"), "tttt"),
        new Date(2024, 0, 10, 9, 5, 7).toLocaleTimeString(Qt.locale("C"), Locale.LongFormat),
        new Date(2024, 0, 10, 9, 5, 7).toLocaleTimeString(Qt.locale("C"), Locale.ShortFormat),
        Qt.locale().name + "/" + Qt.locale("C").name,
    ].map(v => typeof v + ":" + v).join("\n")
}

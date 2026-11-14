// The four formats the shell takes from its settings (qml/services/DateTime.qml), where a
// user may write `t`, `tt`, `ttt` or `tttt`, in translated JS as in the interpreter: the
// expressions of crates/qml/tests/system_zone_names_qt/settings_formats.cases, answered in
// each of its environments and compared with Qt 6.12's answers there.
// answers: ../../../../qml/tests/system_zone_names_qt/settings_formats.cases
import QtQml
QtObject {
    property string result: [
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.format ?? "hh:mm"))({date: new Date(2024, 0, 5, 13, 4, 5)}, {options: {time: {format: "t"}}}),
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.format ?? "hh:mm"))({date: new Date(2024, 0, 5, 13, 4, 5)}, {options: {time: {format: "tt"}}}),
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.format ?? "hh:mm"))({date: new Date(2024, 0, 5, 13, 4, 5)}, {options: {time: {format: "ttt"}}}),
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.format ?? "hh:mm"))({date: new Date(2024, 0, 5, 13, 4, 5)}, {options: {time: {format: "tttt"}}}),
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.format ?? "hh:mm"))({date: new Date(2024, 6, 5, 13, 4, 5)}, {options: {time: {format: "t"}}}),
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.format ?? "hh:mm"))({date: new Date(2024, 6, 5, 13, 4, 5)}, {options: {time: {format: "tt"}}}),
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.format ?? "hh:mm"))({date: new Date(2024, 6, 5, 13, 4, 5)}, {options: {time: {format: "ttt"}}}),
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.format ?? "hh:mm"))({date: new Date(2024, 6, 5, 13, 4, 5)}, {options: {time: {format: "tttt"}}}),
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.shortDateFormat ?? "dd/MM"))({date: new Date(2024, 0, 5, 13, 4, 5)}, {options: {time: {shortDateFormat: "t"}}}),
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.shortDateFormat ?? "dd/MM"))({date: new Date(2024, 0, 5, 13, 4, 5)}, {options: {time: {shortDateFormat: "tt"}}}),
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.shortDateFormat ?? "dd/MM"))({date: new Date(2024, 0, 5, 13, 4, 5)}, {options: {time: {shortDateFormat: "ttt"}}}),
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.shortDateFormat ?? "dd/MM"))({date: new Date(2024, 0, 5, 13, 4, 5)}, {options: {time: {shortDateFormat: "tttt"}}}),
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.shortDateFormat ?? "dd/MM"))({date: new Date(2024, 6, 5, 13, 4, 5)}, {options: {time: {shortDateFormat: "t"}}}),
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.shortDateFormat ?? "dd/MM"))({date: new Date(2024, 6, 5, 13, 4, 5)}, {options: {time: {shortDateFormat: "tt"}}}),
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.shortDateFormat ?? "dd/MM"))({date: new Date(2024, 6, 5, 13, 4, 5)}, {options: {time: {shortDateFormat: "ttt"}}}),
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.shortDateFormat ?? "dd/MM"))({date: new Date(2024, 6, 5, 13, 4, 5)}, {options: {time: {shortDateFormat: "tttt"}}}),
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.dateWithYearFormat ?? "dd/MM/yyyy"))({date: new Date(2024, 0, 5, 13, 4, 5)}, {options: {time: {dateWithYearFormat: "t"}}}),
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.dateWithYearFormat ?? "dd/MM/yyyy"))({date: new Date(2024, 0, 5, 13, 4, 5)}, {options: {time: {dateWithYearFormat: "tt"}}}),
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.dateWithYearFormat ?? "dd/MM/yyyy"))({date: new Date(2024, 0, 5, 13, 4, 5)}, {options: {time: {dateWithYearFormat: "ttt"}}}),
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.dateWithYearFormat ?? "dd/MM/yyyy"))({date: new Date(2024, 0, 5, 13, 4, 5)}, {options: {time: {dateWithYearFormat: "tttt"}}}),
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.dateWithYearFormat ?? "dd/MM/yyyy"))({date: new Date(2024, 6, 5, 13, 4, 5)}, {options: {time: {dateWithYearFormat: "t"}}}),
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.dateWithYearFormat ?? "dd/MM/yyyy"))({date: new Date(2024, 6, 5, 13, 4, 5)}, {options: {time: {dateWithYearFormat: "tt"}}}),
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.dateWithYearFormat ?? "dd/MM/yyyy"))({date: new Date(2024, 6, 5, 13, 4, 5)}, {options: {time: {dateWithYearFormat: "ttt"}}}),
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.dateWithYearFormat ?? "dd/MM/yyyy"))({date: new Date(2024, 6, 5, 13, 4, 5)}, {options: {time: {dateWithYearFormat: "tttt"}}}),
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.dateFormat ?? "dddd, dd/MM"))({date: new Date(2024, 0, 5, 13, 4, 5)}, {options: {time: {dateFormat: "t"}}}),
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.dateFormat ?? "dddd, dd/MM"))({date: new Date(2024, 0, 5, 13, 4, 5)}, {options: {time: {dateFormat: "tt"}}}),
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.dateFormat ?? "dddd, dd/MM"))({date: new Date(2024, 0, 5, 13, 4, 5)}, {options: {time: {dateFormat: "ttt"}}}),
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.dateFormat ?? "dddd, dd/MM"))({date: new Date(2024, 0, 5, 13, 4, 5)}, {options: {time: {dateFormat: "tttt"}}}),
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.dateFormat ?? "dddd, dd/MM"))({date: new Date(2024, 6, 5, 13, 4, 5)}, {options: {time: {dateFormat: "t"}}}),
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.dateFormat ?? "dddd, dd/MM"))({date: new Date(2024, 6, 5, 13, 4, 5)}, {options: {time: {dateFormat: "tt"}}}),
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.dateFormat ?? "dddd, dd/MM"))({date: new Date(2024, 6, 5, 13, 4, 5)}, {options: {time: {dateFormat: "ttt"}}}),
        ((clock, Config) => Qt.locale().toString(clock.date, Config.options?.time.dateFormat ?? "dddd, dd/MM"))({date: new Date(2024, 6, 5, 13, 4, 5)}, {options: {time: {dateFormat: "tttt"}}}),
    ].map(v => typeof v + ":" + v).join("\n")
}

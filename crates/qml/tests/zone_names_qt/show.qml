import QtQml

QtObject {
    // 2024-01-10 12:00:30 UTC in local time: the C locale's names, a locale with names of
    // its own, an English one, and the system's.
    function show() {
        var d = new Date(1704888030000)
        return [
            Qt.formatDateTime(d, 'h:mm t|tt|ttt|tttt'),
            d.toLocaleString(Qt.locale('de_DE'), 't|tt|ttt|tttt'),
            d.toLocaleString(Qt.locale('en_US'), 't'),
            d.toLocaleTimeString(Qt.locale('en_US')),
            d.toLocaleString(Qt.locale(), 't|tt|ttt|tttt'),
            d.toLocaleTimeString()
        ].join('|')
    }
}

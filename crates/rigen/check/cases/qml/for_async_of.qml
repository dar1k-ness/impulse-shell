// QQmlJS has no async functions: `async` in `for (async of xs)` is the name the loop assigns,
// a local, a property of the object, or with a comment or a line break between the tokens.
// The same text in a string, a template and a regular expression stays as written.
// qt: 4 5 122 7 8 9 2 3 for (async of [1])|for (async of 2)|for (async of) 6
import QtQuick
Item {
    id: root
    property var async: 0
    property var b: { for (async of [3, 4]) {} return async }
    property var c: { var async = 1; for (async of [5]) {}; async }
    function f() { var async; var s = ""; for (async of [1, 2]) s += async; return s + async }
    function g() { var async = 0; for ( async /* x */ of [7]) {} return async }
    function h() {
        var async = 0
        for (async // c
             of [8]) {}
        return async
    }
    function i() { var async = 0, of = [9]; for (async of of) {} return async }
    function j() { var async = 0; for (async of [1]) for (async of [2]) {} return async }
    function k() { var async = 0; var m = () => { for (async of [3]) {} }; m(); return async }
    function s() { return ["for (async of [1])", `for (async of ${1 + 1})`, /for (async of)/.source].join("|") }
    property int hits: 0
    Component.onCompleted: { var async; for (async of [6]) hits = async }
    property string result: [b, c, f(), g(), h(), i(), j(), k(), s(), hits].join(" ")
}

// Qt.include (Qt 6.12 QML Global Object): runs the file in this file's context, which gets its
// declarations; URLs resolve against the including file; the result object and the callback.
// v4: 0|2|3|true|boom|1|function|12|1|undefined|undefined|2,function|n1|0|OK,LOADING,NETWORK_ERROR,EXCEPTION,status|OK,LOADING,NETWORK_ERROR,EXCEPTION,status,exception|0,OK,LOADING,NETWORK_ERROR,EXCEPTION,status,2
var log = [];
function libLocal() { return 10; }
var r1 = Qt.include("included/qt_include_inc.js", function (r) { log.push(r.status, Object.keys(r).join()); });
var r2 = Qt.include("included/missing.js", function (r) { log.push(r.status); });
var r3 = Qt.include("included/qt_include_throws.js");
var r4 = Qt.include("included/qt_include_inc.js", function () { throw new Error("cb"); });
var result = [r1.status, r2.status, r3.status, r3.exception instanceof Error, r3.exception.message,
    before, typeof incFn, incFn(2), incVar, typeof incLet, typeof IncClass, incLexical(), nested, r4.status,
    Object.keys(r1).join(), Object.keys(r3).join(), log.join()].join("|");

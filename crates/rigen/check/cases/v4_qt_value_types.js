// Qt.url, Qt.font, Qt.matrix4x4, Qt.quaternion and the vectors their methods take and give:
// QML value types (QQuickValueTypeProvider), compared by value, with their Q_INVOKABLEs.
// v4: ["object","http://example.com/aA%2Fb","http://example.com/aA%2Fb","http://example.com/aA%2Fb","\"http://example.com/aA%2Fb\"",true,true,true,"http://example.com/aA%2Fb","TypeError: Passing incompatible arguments to C++ functions from JavaScript is not allowed.","QMatrix4x4(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16)","QMatrix4x4(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)","QQuaternion(1, 2, 3, 4)","QVector3D(1, 2, 3)","QVector2D(1.5, 2)","QVector4D(1, 2, 3, 4)",true,false,true,true,false,["scalar","x","y","z"],16,"{\"scalar\":1,\"x\":2,\"y\":3,\"z\":4}","{\"x\":1,\"y\":2,\"z\":3}","function","QMatrix4x4(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16)","QMatrix4x4(90, 100, 110, 120, 202, 228, 254, 280, 314, 356, 398, 440, 426, 484, 542, 600)","QMatrix4x4(2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 22, 24, 26, 28, 30, 32)","QVector4D(30, 70, 110, 150)","QVector3D(0.176471, 0.45098, 0.72549)","QMatrix4x4(2, 2, 3, 4, 5, 7, 7, 8, 9, 10, 12, 12, 13, 14, 15, 17)","QMatrix4x4(0, 2, 3, 4, 5, 5, 7, 8, 9, 10, 10, 12, 13, 14, 15, 15)","QVector4D(5, 6, 7, 8)","QVector4D(3, 7, 11, 15)",0,"QMatrix4x4(1, 5, 9, 13, 2, 6, 10, 14, 3, 7, 11, 15, 4, 8, 12, 16)","QMatrix4x4(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)","QMatrix4x4(1.7320507764816284, -1, 0, 1, 1, 1.7320507764816284, 0, 2, 0, 0, 2, 3, 0, 0, 0, 1)","QMatrix4x4(0.4330126941204071, 0.25, 0, -0.9330127239227295, -0.25, 0.4330126941204071, 0, -0.6160253882408142, 0, 0, 0.5, -1.5, 0, 0, 0, 1)",7.999999784621224,"QMatrix4x4(0, 0, 3, 0, 1, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 1)","QMatrix4x4(0.9486833214759827, 0, -0.3162277638912201, -5.960464477539063e-08, -0.16903084516525269, 0.8451542854309082, -0.5070925354957581, 0, 0.26726123690605164, 0.5345224738121033, 0.8017837405204773, -3.741657257080078, 0, 0, 0, 1)","{\"x\":1.7320507764816284,\"y\":4.732050776481628}",[-1,2,5.464101791381836,5.464101791381836],true,false,true,false,20,"QQuaternion(-3.5, -8, -0.5, 9)","QVector3D(-20, 20, 10)","QQuaternion(2, 4, 6, 8)","QQuaternion(2, 4, 6, 8)","QQuaternion(0, 0, 0, 0)","QQuaternion(0.182574, 0.365148, 0.547723, 0.730297)","QQuaternion(0.0333333, -0.0666667, -0.1, -0.133333)","QQuaternion(1, -2, -3, -4)",5.4772257804870605,"QVector3D(-41.8103, 79.6952, 116.565)","QVector4D(2, 3, 4, 1)",true,true,true,false,{"scalar":null,"x":null,"y":null,"z":null},"Error: Unable to determine callable overload.  Candidates are:\n    times(double)\n    times(QVector3D)\n    times(QQuaternion)","Error: Insufficient arguments","QFont(Sans,10,-1,5,700,1,0,0,0,0,0,0,0,0,0,1,,0,0)",10,13,true,700,true,"Sans","QFont(Sans,-1,20,5,300,0,0,0,0,0,0,1,1.5,0,0,1,,0,0)",15,20,false,1.5,true,false,"family,styleName,bold,weight,italic,underline,overline,strikeout,pointSize,pixelSize,capitalization,letterSpacing,wordSpacing,hintingPreference,kerning,preferShaping,features,variableAxes,contextFontMerging,preferTypoLineMetrics","object"]
function err(f) { try { return f() } catch (e) { return e.name + ": " + e.message } }
var u = Qt.url("HTTP://Example.COM/a%41%2Fb");
var m = Qt.matrix4x4(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16);
var id = Qt.matrix4x4();
var q = Qt.quaternion(1, 2, 3, 4);
var v3 = Qt.vector3d(1, 2, 3);
var t = Qt.matrix4x4();
t.translate(Qt.vector3d(1, 2, 3));
t.rotate(30, Qt.vector3d(0, 0, 1));
t.scale(2);
var r = Qt.matrix4x4();
r.rotate(Qt.quaternion(0.5, 0.5, 0.5, 0.5));
r.scale(1, 2, 3);
var la = Qt.matrix4x4();
la.lookAt(Qt.vector3d(1, 2, 3), Qt.vector3d(0, 0, 0), Qt.vector3d(0, 1, 0));
var f = Qt.font({ family: "Sans", pointSize: 10.5, bold: true, italic: true });
var g = Qt.font({ family: "Sans", pixelSize: 20, letterSpacing: 1.5, weight: 300 });
var mr = t.mapRect(Qt.rect(0, 0, 2, 2));
var keys = []; for (var k in q) keys.push(k);
var result = JSON.stringify([
  typeof u, String(u), u.toString(), u.valueOf(), JSON.stringify(u), u == Qt.url("http://example.com/aA%2Fb"),
  u === Qt.url("http://example.com/aA%2Fb"), u == "http://example.com/aA%2Fb", Qt.url(u).toString(),
  err(function () { return Qt.url(1) }),
  String(m), String(id), String(q), String(v3), String(Qt.vector2d(1.5, 2)), String(Qt.vector4d(1, 2, 3, 4)),
  m === Qt.matrix4x4(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16), m == id, q === Qt.quaternion(1, 2, 3, 4),
  v3 === Qt.vector3d(1, 2, 3), Qt.vector3d(0, 0, 0) == Qt.quaternion(0, 0, 0, 0), keys, Object.keys(m).length,
  JSON.stringify(q), JSON.stringify(v3), typeof q.times,
  String(m.times(id)), String(m.times(m)), String(m.times(2)), String(m.times(Qt.vector4d(1, 2, 3, 4))),
  String(m.times(Qt.vector3d(1, 2, 3))), String(m.plus(id)), String(m.minus(id)), String(m.row(1)),
  String(m.column(2)), m.determinant(), String(m.transposed()), String(m.inverted()),
  String(t), String(t.inverted()), t.determinant(), String(r), String(la),
  JSON.stringify(t.map(Qt.point(1, 1))), [mr.x, mr.y, mr.width, mr.height],
  m.fuzzyEquals(m), m.fuzzyEquals(id), m.fuzzyEquals(m.plus(id), 1), m.fuzzyEquals(m.plus(id), 0.5),
  q.dotProduct(Qt.quaternion(4, 3, 2, 1)), String(q.times(Qt.quaternion(0.5, -1, 2, 0))),
  String(q.times(Qt.vector3d(1, 0, 0))), String(q.times(2)), String(q.plus(q)), String(q.minus(q)),
  String(q.normalized()), String(q.inverted()), String(q.conjugated()), q.length(),
  String(q.normalized().toEulerAngles()), String(q.toVector4d()), q.fuzzyEquals(q), q.fuzzyEquals(q.times(1.0000001)),
  q.fuzzyEquals(Qt.quaternion(1.1, 2, 3, 4), 0.2), q.fuzzyEquals(Qt.quaternion(1.3, 2, 3, 4), 0.2),
  err(function () { return q.times("x") }), err(function () { return q.times() }), err(function () { return m.row() }),
  String(f), f.pointSize, f.pixelSize, f.bold, f.weight, f.italic, f.family, String(g), g.pointSize, g.pixelSize,
  g.bold, g.letterSpacing, f == Qt.font({ family: "Sans", pointSize: 10, bold: true, italic: true }), f === g,
  Object.keys(f).join(), typeof f.features
]);

// Qt.point/size/rect (QtQml QPointF/QSizeF/QRectF value types: toString, equality by value,
// QRectF edges) and the Q_INVOKABLEs of Qt.vector2d/3d/4d (QQuickVector2D/3D/4DValueType).
// v4: [["QRectF(1, 2, 3, 4)","QSizeF(1, 2)","QPointF(1.5, 2)","QPointF(1.23457e+06, 1e-07)",true,true,true,false,false,false,["x","y","width","height","left","right","top","bottom"],["x","y"],"{\"x\":1,\"y\":2,\"width\":3,\"height\":4,\"left\":1,\"right\":4,\"top\":2,\"bottom\":6}","{\"x\":1.5,\"y\":2}","{\"width\":1,\"height\":2}",1,4,2,6,"QRectF(10, 2, 3, 4)","Error: Cannot assign double to double","Error: Cannot assign double to double","QRectF(10, 2, 0.1, 4)",10,10.1,"object","function",1.5,"Error: Insufficient arguments","Error: Insufficient arguments","Error: Insufficient arguments","Error: Too many arguments","Error: Too many arguments","QPointF(nan, 0)","QPointF(3, 1)",0.1,false,false,false,true,true,["x","y"],"QRectF(-1, -2, -3, -4)",-4,true,true,"QPointF(nan, inf)","QPointF(1e+21, 1e-06)"],["QPointF(7, 2)",null,0.1,"QSizeF(0.1, 3)",0.10000000149011612,[5,6]],[11,"QVector2D(3, 8)","QVector2D(7.5, 10)","QVector2D(4, 6)","QVector2D(2, 2)","QVector2D(0.6, 0.8)",5,"QVector3D(3, 4, 0)","QVector4D(3, 4, 0, 0)",false,true,false,true,"QVector3D(-3, 6, -3)",32,"QVector3D(0.708333, 0.805556, 0.902778)","QVector3D(0.124402, 0.0821367, 0.2)","QVector3D(4, 10, 18)","QVector3D(3, 6, 9)","QVector3D(5, 7, 9)","QVector3D(-3, -3, -3)","QVector3D(0.267261, 0.534522, 0.801784)",3.741657257080078,"QVector2D(1, 2)","QVector4D(1, 2, 3, 0)",true,true,false,10,"QVector4D(90, 100, 110, 120)","QVector4D(1, 2, 3, 4)","QVector4D(0.5, 1, 1.5, 2)","QVector4D(2, 3, 4, 5)","QVector4D(0, 1, 2, 3)","QVector4D(0.182574, 0.365148, 0.547723, 0.730297)",5.4772257804870605,"QVector2D(1, 2)","QVector3D(1, 2, 3)",true,true,"QVector2D(0, 0)","QVector3D(0, 0, 0)","QVector4D(0, 0, 0, 0)",9.999999974752427e-7,"QVector3D(0, 0, 0)",["x","y","z"],"function",false,"Error: Insufficient arguments","{\"x\":null,\"y\":null,\"z\":null}","Error: Unable to determine callable overload.  Candidates are:\n    fuzzyEquals(QVector2D)\n    fuzzyEquals(QVector2D,double)","{\"x\":4,\"y\":5}",1.100000023841858,"TypeError: Property 'crossProduct' of object QVector2D(3, 4) is not a function","QVector2D(0.707107, 0.707107)",1.4142135130433894e+30,"Error: Insufficient arguments","Error: Too many arguments"],["QVector3D(1, 2, 3)","QVector3D(1, 2, 3)","QVector3D(5, 7, 9)","QVector3D(2, 2, 3)","TypeError: Passing incompatible arguments to C++ functions from JavaScript is not allowed.","TypeError: Passing incompatible arguments to C++ functions from JavaScript is not allowed.",6,"QVector2D(2, 3)","QPointF(0.157895, 0.438596)","QRectF(0.125, 0.416667, 0.0328947, 0.0219298)","{\"x\":1,\"y\":2,\"z\":0}"]]
function err(f) { try { return f() } catch (e) { return e.name + ": " + e.message } }
var r = Qt.rect(1, 2, 3, 4), p = Qt.point(1.5, 2), s = Qt.size(1, 2);
var keys = []; for (var k in r) keys.push(k);
var r2 = Qt.rect(1, 2, 3, 4); r2.x = 10;
var moved = String(r2);
var lerr = err(function () { r2.left = 99; return "ok" });
r2.width = 0.1;
var geometry = [
  String(r), String(s), String(p), String(Qt.point(1234567.5, 1e-7)), r == Qt.rect(1, 2, 3, 4), p === Qt.point(1.5, 2),
  s == Qt.size(1, 2), r == Qt.rect(1, 2, 3, 5), r != Qt.rect(1, 2, 3, 4), Qt.rect(1, 2, 3, 4) == Qt.rect(1, 2, 3, 4.0000001),
  keys, Object.keys(p), JSON.stringify(r), JSON.stringify(p), JSON.stringify(s), r.left, r.right, r.top, r.bottom,
  moved, lerr, err(function () { r2.right = "x" }), String(r2), r2.left, r2.right, typeof r, typeof r.toString, p.x,
  err(function () { return String(Qt.point()) }), err(function () { return String(Qt.rect(1, 2, 3)) }),
  err(function () { return String(Qt.size(1)) }), err(function () { return String(Qt.point(1, 2, 3)) }),
  err(function () { return String(Qt.size(1, 2, 3)) }), String(Qt.point(undefined, null)), String(Qt.point("3", true)),
  Qt.point(0.1, 0.2).x, p == Qt.vector2d(1.5, 2), Qt.point(1, 2) == Qt.vector2d(1, 2), Object.getPrototypeOf(p) === Object.prototype,
  "x" in p, p.hasOwnProperty("x"), Object.getOwnPropertyNames(p), String(Qt.rect(-1, -2, -3, -4)), Qt.rect(-1, -2, -3, -4).right,
  Qt.size(-1, -1) == Qt.size(-1, -1), p == "QPointF(1.5, 2)", String(Qt.point(NaN, Infinity)), String(Qt.point(1e21, 0.000001))
];
var q = Qt.point(1, 2);
q.x = "7"; q.foo = 1; delete q.x;
var sz = Qt.size(1, 2); sz.width = 0.1; sz.height = "3";
var vv = Qt.vector2d(1, 2); vv.x = 0.1;
var ry = Qt.rect(0, 0, 1, 1); ry.y = 5;
var writes = [String(q), q.foo, sz.width, String(sz), vv.x, [ry.top, ry.bottom]];
var a2 = Qt.vector2d(3, 4), b2 = Qt.vector2d(1, 2);
var a3 = Qt.vector3d(1, 2, 3), b3 = Qt.vector3d(4, 5, 6);
var a4 = Qt.vector4d(1, 2, 3, 4), b4 = Qt.vector4d(1, 1, 1, 1);
var m = Qt.matrix4x4(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16);
var t = Qt.matrix4x4(); t.translate(Qt.vector3d(1, 2, 3)); t.rotate(30, Qt.vector3d(0, 0, 1));
var vkeys = []; for (var k3 in a3) vkeys.push(k3);
var vectors = [
  a2.dotProduct(b2), String(a2.times(b2)), String(a2.times(2.5)), String(a2.plus(b2)), String(a2.minus(b2)),
  String(a2.normalized()), a2.length(), String(a2.toVector3d()), String(a2.toVector4d()),
  a2.fuzzyEquals(b2), a2.fuzzyEquals(Qt.vector2d(3.1, 4), 0.2), a2.fuzzyEquals(Qt.vector2d(3.3, 4), 0.2), a2.fuzzyEquals(Qt.vector2d(3.00001, 4)),
  String(a3.crossProduct(b3)), a3.dotProduct(b3), String(a3.times(m)), String(a3.times(t)), String(a3.times(b3)), String(a3.times(3)),
  String(a3.plus(b3)), String(a3.minus(b3)), String(a3.normalized()), a3.length(), String(a3.toVector2d()), String(a3.toVector4d()),
  a3.fuzzyEquals(a3), a3.fuzzyEquals(b3, 3), a3.fuzzyEquals(b3, 2.9),
  a4.dotProduct(b4), String(a4.times(m)), String(a4.times(b4)), String(a4.times(0.5)), String(a4.plus(b4)), String(a4.minus(b4)),
  String(a4.normalized()), a4.length(), String(a4.toVector2d()), String(a4.toVector3d()), a4.fuzzyEquals(a4), a4.fuzzyEquals(b4, 3),
  String(Qt.vector2d(0, 0).normalized()), String(Qt.vector3d(0, 0, 0).normalized()), String(Qt.vector4d(0, 0, 0, 0).normalized()),
  Qt.vector3d(1e-6, 0, 0).length(), String(Qt.vector3d(1, 2, 3).times(Qt.matrix4x4(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0))),
  vkeys, typeof a3.plus, a3.plus === a3.plus, err(function () { return a3.plus() }),
  JSON.stringify(a3.times("x")), err(function () { return a2.fuzzyEquals() }), JSON.stringify(a2.plus(Qt.vector3d(1, 1, 1))), a2.dotProduct(Qt.vector2d(0.1, 0.2)),
  err(function () { return a2.crossProduct(b2) }), String(Qt.vector2d(1e30, 1e30).normalized()), Qt.vector2d(1e30, 1e30).length(),
  err(function () { return String(Qt.vector3d(1, 2)) }), err(function () { return String(Qt.vector3d(1, 2, 3, 4)) })
];
var conversions = [
  String(a3.plus("abc")), String(a3.plus({})), String(a3.plus("4,5,6")), String(a3.plus({ x: 1 })),
  err(function () { return String(a3.plus(null)) }),
  err(function () { return String(a3.plus(5)) }), a3.dotProduct(Qt.vector4d(1, 1, 1, 1)), String(Qt.vector2d(1, 2).plus(Qt.point(1, 1))),
  String(m.map(Qt.point(1, 2))), String(m.mapRect(Qt.rect(1, 2, 3, 4))), JSON.stringify(Qt.vector2d(1, 2).toVector3d())
];
var result = JSON.stringify([geometry, writes, vectors, conversions]);

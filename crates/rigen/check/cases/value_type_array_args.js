// Where native deliberately differs from Qt 6.12 (no `// v4:` pin): an array passed for a
// value type parameter or stored in a point/size/rect is its components (Qt makes it the
// type's default value), and a value type's methods are found by `in` as by a read.
var a2 = Qt.vector2d(3, 4), a3 = Qt.vector3d(1, 2, 3), a4 = Qt.vector4d(1, 2, 3, 4);
var m = Qt.matrix4x4(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16);
var result = JSON.stringify([
  String(a3.plus([1, 1, 1])), String(a3.times([1, 2, 3])), String(a4.times([1, 2, 3, 4])), String(a2.times([1, 2])),
  String(a3.plus([1, 2])), String(m.times([1, 2, 3])), String(Qt.quaternion(1, 2, 3, 4).plus([1, 2])),
  String(Qt.vector2d(1, 2).plus([5, 6])), String(Qt.vector4d(1, 2, 3, 4).plus([1, 1, 1, 1])),
  String(m.plus([1, 0, 0, 0, 0, 2, 0, 0, 0, 0, 3, 0, 0, 0, 0, 1])), Qt.vector3d(1, 2, 3).dotProduct([1, 1, 1]),
  String(m.map([3, 4])), "plus" in a3, "times" in m, "crossProduct" in a2
]);

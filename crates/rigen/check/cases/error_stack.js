// Errors have an own `stack` string (not enumerable), thrown ones too.
// v4: ["string",true,0,"string","string","string"]
var e = new Error("m"), t = new TypeError("x"), caught;
try { null.x } catch (x) { caught = x }
var r = [typeof e.stack, e.hasOwnProperty("stack"), Object.keys(e).length, typeof t.stack, typeof caught.stack,
    typeof new RangeError().stack];
var result = JSON.stringify(r);

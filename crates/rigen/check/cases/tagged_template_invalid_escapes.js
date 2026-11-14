// ECMA-262 §13.2.8.3: an invalid escape is legal in a tagged template, whose cooked string is
// then undefined (raw keeps the source). Qt 6.12 V4 rejects these with a SyntaxError, so no pin.
function tag(strs) {
    var out = [strs.length, strs.raw.length];
    for (var i = 0; i < strs.length; i++) out.push(strs[i] === undefined ? "U" : strs[i], strs.raw[i]);
    for (var j = 1; j < arguments.length; j++) out.push(arguments[j]);
    return out.join("|");
}
var result = JSON.stringify([tag`bad\unicode ok\xg ${3} \u{110000}`, tag`\01`, tag`\u{`, tag`ok`]);

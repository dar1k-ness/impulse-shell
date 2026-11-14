// Where V4 6.12 deviates from ECMA-262, hence no `// v4:` pin: Date.parse of strings that only
// look like the date-time string format (§21.4.1.32: hour 24 with nonzero minutes, seconds or
// milliseconds, the year -000000), which V4 accepts, and Date.prototype[@@toPrimitive] being
// configurable (§21.4.4.45), which it is not in V4.
var result = [
    Date.parse("2020-02-29T24:00:01Z"),
    Date.parse("2020-02-29T24:01Z"),
    Date.parse("2020-02-29T24:00:00.001Z"),
    Date.parse("-000000-01-01T00:00:00Z"),
    Object.getOwnPropertyDescriptor(Date.prototype, Symbol.toPrimitive).configurable
].join("|");

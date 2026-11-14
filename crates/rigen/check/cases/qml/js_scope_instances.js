var own = 0
function bump() { own += 1; return own }
function get() { return own }
function who() { return typeof tag === "undefined" ? "none" : tag }
function w() { return typeof width === "undefined" ? "none" : width }
function idx() { return typeof index === "undefined" ? "none" : index }
function md() { return typeof modelData === "undefined" ? "none" : modelData }
function rootId() { return typeof root === "undefined" ? "none" : root.tag }
function cellId() { return typeof cell === "undefined" ? "none" : cell.tag }

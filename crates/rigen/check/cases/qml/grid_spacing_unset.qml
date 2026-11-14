// Grid.rowSpacing and Grid.columnSpacing read -1 until they are written, whatever `spacing`
// is. A write stores the value (and tells, unless it is the stored one); the reset
// (`undefined`) leaves the stored value and tells nothing.
// qt: 0/-1/-1 4/-1/-1 0/3/-1 4/-1/5 0/-1/0 g 0/-1/-1 -11 6/-1/-1 -11 6/2/-1 19 1/2/-1 1/2/-1 19 1/2/7 27 1/-1/7 -3 1/-1/-5 -15 srscrc
import QtQuick

Item {
    id: root
    property string result

    Grid { id: a }
    Grid { id: b; spacing: 4 }
    Grid { id: c; rowSpacing: 3 }
    Grid { id: d; spacing: 4; columnSpacing: 5 }
    Grid { id: e; rowSpacing: -1; columnSpacing: 0 }
    Grid {
        id: g
        property string log
        onSpacingChanged: log += "s"
        onRowSpacingChanged: log += "r"
        onColumnSpacingChanged: log += "c"
    }
    property real bound: g.rowSpacing * 10 + g.columnSpacing

    function sp(x) { return x.spacing + "/" + x.rowSpacing + "/" + x.columnSpacing }

    Component.onCompleted: {
        let r = [sp(a), sp(b), sp(c), sp(d), sp(e), "g", sp(g), bound]
        g.spacing = 6
        r.push(sp(g), bound)
        g.rowSpacing = 2
        r.push(sp(g), bound)
        g.spacing = 1
        r.push(sp(g))
        g.rowSpacing = undefined
        r.push(sp(g), bound)
        g.rowSpacing = 2
        g.columnSpacing = 7
        r.push(sp(g), bound)
        g.columnSpacing = undefined
        g.rowSpacing = -1
        r.push(sp(g), bound)
        g.rowSpacing = -1
        g.columnSpacing = -5
        r.push(sp(g), bound, g.log)
        result = r.join(" ")
    }
}

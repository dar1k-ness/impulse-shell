// A binding that reads the property it is bound to reads what the property holds
// (QQmlBinding::update runs the expression before anything is written), and is not run again
// by its own write when that leaves the value as it was: the property sends no signal, so
// there is no binding loop. Each row: the value, how often the binding ran, what it read
// from its own property each time, how often the change signal came; before and after
// `src` changes.
// qt: a=0 ran 1 read 0 signals 0; d=0 ran 1 read 0 signals 0; j=0 ran 1 read 0 signals 0; k=-1 ran 1 read -1 signals 0; l=-1 ran 1 read -1 signals 0; f=32 ran 1 read - signals 1; h=32 ran 1 read - signals 1; m=10 ran 1 read - signals 1 | a=0 ran 1 read 0 signals 0; d=0 ran 1 read 0 signals 0; j=0 ran 1 read 0 signals 0; k=-1 ran 2 read -1,-1 signals 0; l=-1 ran 2 read -1,-1 signals 0; f=32 ran 1 read - signals 1; h=32 ran 1 read - signals 1; m=10 ran 1 read - signals 1
import QtQuick

Item {
    id: root
    width: 300
    height: 400
    property var n: ({})
    property int src: 10
    property string result

    function tick(k, seen) {
        n[k] = (n[k] || 0) + 1;
        n[k + "s"] = (n[k + "s"] || "") + (n[k + "s"] ? "," : "") + String(seen);
    }
    function sig(k) {
        n[k + "c"] = (n[k + "c"] || 0) + 1;
    }
    function row(k, v) {
        return k + "=" + v + " ran " + (n[k] || 0) + " read " + (n[k + "s"] || "") + " signals " + (n[k + "c"] || 0);
    }
    function rows() {
        return [row("a", a.height), row("d", d.p), row("j", j.height), row("k", k.contentWidth),
            row("l", l.columns), row("f", f.height), row("h", h.height), row("m", m.width)].join("; ");
    }

    // The value it read.
    Item {
        id: a
        height: {
            root.tick("a", height);
            return Math.min(height, 100);
        }
        onHeightChanged: root.sig("a")
    }
    // A property of the document.
    Item {
        id: d
        property real p: {
            root.tick("d", p);
            return Math.min(p, 5);
        }
        onPChanged: root.sig("d")
    }
    // Through an alias.
    Item {
        id: j
        property alias mine: j.height
        height: {
            root.tick("j", mine);
            return Math.min(mine, 100);
        }
        onHeightChanged: root.sig("j")
    }
    // A property that refuses `undefined` and holds -1 when nothing was written.
    Flickable {
        id: k
        contentWidth: {
            root.tick("k", contentWidth);
            return root.src > 10 ? contentWidth : undefined;
        }
        onContentWidthChanged: root.sig("k")
    }
    Grid {
        id: l
        columns: {
            root.tick("l", columns);
            return root.src > 10 ? columns : undefined;
        }
        onColumnsChanged: root.sig("l")
    }
    // A view sized from its content item, whose size is the view's less its margins
    // (QQuickFlickable::geometryChange): the content item stays 0 high, so nothing the
    // binding read has changed.
    Flickable {
        id: f
        topMargin: 16
        bottomMargin: 16
        height: {
            root.tick("f", "-");
            return Math.min(contentItem.height + topMargin + bottomMargin, parent.height);
        }
        onHeightChanged: root.sig("f")
    }
    // A view with nothing in it: its content item is as long as its content, 0.
    ListView {
        id: h
        topMargin: 16
        bottomMargin: 16
        height: {
            root.tick("h", "-");
            return Math.min(contentItem.height + topMargin + bottomMargin, parent.height);
        }
        onHeightChanged: root.sig("h")
    }
    ListView {
        id: m
        orientation: ListView.Horizontal
        leftMargin: 5
        rightMargin: 5
        width: {
            root.tick("m", "-");
            return Math.min(contentItem.width + leftMargin + rightMargin, parent.width);
        }
        onWidthChanged: root.sig("m")
    }

    Component.onCompleted: {
        var first = rows();
        src = 20;
        result = first + " | " + rows();
    }
}

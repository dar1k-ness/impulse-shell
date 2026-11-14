import QtQuick

// A binding that reads the property it is bound to and writes another value than it read:
// the write notifies the binding while it is still running, Qt logs `Binding loop detected
// for property` and skips that run (QQmlBinding::update, updatingFlag), so the value the
// first run wrote stays. Each row: the value, how often the binding ran, what it read from
// its own property each time, how often the change signal came; before and after `src`
// changes.
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
        return [row("b", b.height), row("c", c.width), row("e", e.q), row("i", i.x), row("g", g.height)].join("; ");
    }

    // Another value than it read.
    Item {
        id: b
        height: {
            root.tick("b", height);
            return Math.max(height, 50);
        }
        onHeightChanged: root.sig("b")
    }
    // Its own value and a source.
    Item {
        id: c
        width: {
            root.tick("c", width);
            return Math.min(width, 100) * 0 + root.src;
        }
        onWidthChanged: root.sig("c")
    }
    // A `var` property of the document.
    Item {
        id: e
        property var q: {
            root.tick("e", q);
            return q === undefined ? 1 : q;
        }
        onQChanged: root.sig("e")
    }
    // Through a function.
    Item {
        id: i
        function own() {
            return i.x;
        }
        x: {
            root.tick("i", "-");
            return Math.min(own(), 100) + root.src;
        }
        onXChanged: root.sig("i")
    }
    // A view sized from its content item, which grows with it.
    Flickable {
        id: g
        height: {
            root.tick("g", "-");
            return Math.min(contentItem.height + 10, parent.height);
        }
        onHeightChanged: root.sig("g")
    }

    Component.onCompleted: {
        var first = rows();
        src = 20;
        result = first + " | " + rows();
    }
}

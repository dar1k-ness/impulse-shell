// The bindings of a creation: the ones of `x`, `y`, `width`, `height` (QQmlPropertyBinding)
// are set after every other one was enabled, in the order the creation made them, and a read
// does not run one early; one whose property was written by then is not set at all.
// Qt 6.12: /opt/qv4-venv/bin/python ../warning_order/qtmsg.py phases.qml
import QtQuick

Item {
    id: root
    property int a: {
        console.log("a");
        return 5;
    }
    width: {
        console.log("root.width, a", a);
        return a * 20;
    }

    Item {
        id: inner
        property int b: {
            console.log("b, inner.width", inner.width);
            return inner.width + 1;
        }
        width: {
            console.log("inner.width, other.width", other.width);
            return 10 + other.width;
        }
        height: {
            console.log("inner.height");
            return 4;
        }
        onBChanged: {
            console.log("inner.onBChanged", b);
            if (b == 1)
                third.x = 9;
        }
    }

    Item {
        id: other
        width: {
            console.log("other.width, a", root.a);
            return root.a;
        }
        x: {
            console.log("other.x");
            return 3;
        }
        y: {
            console.log("other.y, x", x);
            return x + 1;
        }
        onYChanged: console.log("other.onYChanged", y)
    }

    Item {
        id: third
        x: {
            console.log("third.x");
            return 4;
        }
        onXChanged: console.log("third.onXChanged", x)
    }

    Component.onCompleted: {
        console.log("completed", width, inner.width, inner.height, inner.b, other.width, other.y,
                    third.x);
        Qt.quit();
    }
}

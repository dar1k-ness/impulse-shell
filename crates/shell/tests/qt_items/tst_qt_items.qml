import QtQuick
import QtTest

// The values ../qt_items.rs asserts, checked in Qt 6.12 itself. In the tools/testenv image:
//   QT_QPA_PLATFORM=wayland /usr/lib/qt6/bin/qmltestrunner -input crates/shell/tests/qt_items/tst_qt_items.qml
Item {
    width: 240; height: 260

    Paint { id: paint }
    Input { id: input; y: 150 }

    TestCase {
        name: "qt_items"
        when: windowShown

        function px(img, x, y) {
            var c = img.pixel(x, y)
            return [Math.round(c.r * 255), Math.round(c.g * 255), Math.round(c.b * 255)].join(",")
        }

        // First and last column with ink in the rows of text `row`, relative to the text's x.
        function ink(img, row) {
            var first = -1, last = -1
            for (var x = 0; x < 100; x++)
                for (var y = 0; y < 20; y++)
                    if (px(img, 140 + x, 60 + 20 * row + y) !== "0,0,0") {
                        if (first < 0)
                            first = x
                        last = x
                    }
            return first + ".." + last
        }

        function test_paint() {
            var img = grabImage(paint)
            compare(px(img, 25, 15), "100,50,20", "parent opacity")
            compare(px(img, 25, 25), "50,25,10", "parent opacity times the child's")
            compare(px(img, 80, 30), "0,255,0", "inside the clip")
            compare([px(img, 65, 30), px(img, 95, 30), px(img, 80, 15), px(img, 80, 45)].join(" "),
                    "0,0,0 0,0,0 0,0,0 0,0,0", "outside the clip")
            compare([px(img, 65, 90), px(img, 95, 90), px(img, 80, 75), px(img, 80, 105)].join(" "),
                    "0,255,0 0,255,0 0,255,0 0,255,0", "without clip")
            compare([px(img, 143, 13), px(img, 149, 19)].join(" "), "0,0,0 255,255,255", "topLeftRadius 20")
            compare(px(img, 178, 11), "255,255,255", "topRightRadius 0")
            compare([px(img, 141, 47), px(img, 145, 44)].join(" "), "0,0,0 255,255,255", "bottomLeftRadius 10")
            compare([px(img, 179, 49), px(img, 176, 46)].join(" "), "0,0,0 255,255,255", "radius 6 bottom right")
            compare([ink(img, 0), ink(img, 1), ink(img, 2), ink(img, 3)].join(" "),
                    "0..64 1..66 0..61 0..99", "elide: right, left, middle, none")
        }

        function test_input() {
            mouseClick(input, 30, 30)
            mouseClick(input, 80, 30)
            mouseClick(input, 80, 75)
            compare(input.log, "clipped back unclipped ", "clip in click targeting")
            input.log = ""
            // QQuickItem::isEnabled is false under a disabled parent; MouseArea.enabled is a
            // property of its own (QQuickMouseArea::isEnabled), which stays as set.
            compare([input.plain.enabled, input.inner.enabled].join(" "), "false true",
                    "enabled under a disabled parent")
            mouseClick(input, 170, 30)
            input.gate.enabled = true
            compare([input.plain.enabled, input.inner.enabled].join(" "), "true true")
            mouseClick(input, 170, 30)
            compare(input.log, "back inner ", "a disabled parent disables its children")
            input.log = ""
            compare(input.under.enabled, true, "enabled in a disabled MouseArea")
            mouseClick(input, 160, 75)
            mouseClick(input, 180, 75)
            compare(input.log, "under back ", "a disabled MouseArea does not disable its children")
            input.log = ""
            input.ping()
            input.conn.enabled = true
            input.ping()
            input.conn.enabled = false
            input.ping()
            // Enabled later, it is connected later: its handler runs after the other's.
            compare(input.log, "on on off on ", "Connections.enabled")
        }
    }
}

import QtQuick
import QtTest

// The values ../transform_live.rs asserts for Handlers.qml, checked in Qt 6.12 itself. In the
// tools/testenv image:
//   QT_QPA_PLATFORM=wayland /usr/lib/qt6/bin/qmltestrunner -input crates/shell/tests/transform_live/tst_transform_handlers.qml
Item {
    width: 300; height: 200

    Handlers { id: live }

    TestCase {
        name: "transform_handlers"
        when: windowShown

        function took() {
            var l = live.log.trim()
            live.log = ""
            return l
        }
        function card() { return live.r(live.card.x) + "," + live.r(live.card.y) }

        function test_1_drag_handler_moves_its_target() {
            mousePress(live, 30, 30)
            mouseMove(live, 30, 36)
            compare(card(), "0,0")
            // Past the drag threshold the card keeps the pressed point under the pointer.
            mouseMove(live, 30, 52)
            compare(card(), "22,0")
            mouseMove(live, 35, 62)
            compare(card(), "32,-5")
            mouseMove(live, 25, 72)
            compare(card(), "42,5")
            mouseMove(live, 10, 150)
            compare(card(), "70,8")
            mouseRelease(live, 10, 150)
            compare(card(), "70,8")
        }

        function test_2_drag_handler_without_a_target() {
            mousePress(live, 130, 40)
            compare(took(), "")
            mouseMove(live, 130, 46)
            compare(took(), "")
            mouseMove(live, 150, 46)
            compare(took(), "free true")
            mouseMove(live, 160, 70)
            compare(took(), "x 30 y 30")
            mouseRelease(live, 160, 70)
            compare(took(), "free false")
        }

        function test_3_wheel_handler() {
            mouseWheel(live, 245, 150, 0, -120)
            compare(took(), "wheel -120 at 45,15")
            // Where the item is laid out, not where it shows.
            mouseWheel(live, 280, 75, 0, 120)
            compare(took(), "")
            mouseWheel(live, 235, 62, 0, 120)
            compare(took(), "wheel 120 at 1,25")
        }

        function small() { return live.r(live.small.x) + "," + live.r(live.small.y) }

        function test_4_drag_handler_moves_a_scaled_target() {
            mousePress(live, 230, 25)
            mouseMove(live, 230, 27)
            compare(small(), "200,5")
            // 6 in the window is 12 in the target, which is past the drag threshold.
            mouseMove(live, 230, 31)
            compare(small(), "200,11")
            mouseMove(live, 230, 45)
            compare(small(), "200,25")
            mouseMove(live, 220, 55)
            compare(small(), "190,35")
            mouseRelease(live, 220, 55)
            compare(small(), "190,35")
        }

        function widget() { return live.r(live.widget.x) + "," + live.r(live.widget.y) }

        function test_5_handlers_in_a_pane_a_little_over_its_size() {
            mouseWheel(live, 86, 149, 0, 120)
            compare(took(), "zoom 120")
            // Left of the widget as it shows, inside it as it is laid out.
            mouseWheel(live, 60, 149, 0, 120)
            compare(took(), "")
            mousePress(live, 86, 149)
            mouseMove(live, 86, 159)
            compare(widget(), "10,10")
            // 27 in the window is 25 in the pane.
            mouseMove(live, 113, 149)
            compare(widget(), "35,10")
            mouseMove(live, 140, 160)
            compare(widget(), "60,20.2")
            mouseMove(live, 290, 100)
            compare(widget(), "110,0")
            mouseMove(live, 10, 199)
            compare(widget(), "0,40")
            mouseRelease(live, 10, 199)
            compare(widget(), "0,40")
        }
    }
}

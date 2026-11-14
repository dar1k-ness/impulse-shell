import QtQuick
import QtTest

// The values ../transform_live.rs asserts for Drags.qml, checked in Qt 6.12 itself. In the
// tools/testenv image:
//   QT_QPA_PLATFORM=wayland /usr/lib/qt6/bin/qmltestrunner -input crates/shell/tests/transform_live/tst_transform_drags.qml
Item {
    width: 300; height: 200

    Drags { id: live }

    TestCase {
        name: "transform_drags"
        when: windowShown

        function xy(item) { return live.r(item.x) + "," + live.r(item.y) }

        // Presses at the first point, moves through the others and lets go at the last:
        // where `item` is after each move and after the release.
        function drag(item, pts) {
            mousePress(live, pts[0][0], pts[0][1])
            var out = []
            for (var k = 1; k < pts.length; k++) {
                mouseMove(live, pts[k][0], pts[k][1])
                out.push(xy(item))
            }
            mouseRelease(live, pts[pts.length - 1][0], pts[pts.length - 1][1])
            out.push(xy(item))
            return out.join(" ")
        }
        // The same by a finger. Qt delivers a touch move with the next frame.
        function touch(item, pts) {
            var t = touchEvent(live)
            t.press(0, live, pts[0][0], pts[0][1]).commit()
            wait(50)
            var out = []
            for (var k = 1; k < pts.length; k++) {
                t.move(0, live, pts[k][0], pts[k][1]).commit()
                wait(50)
                out.push(xy(item))
            }
            t.release(0, live, pts[pts.length - 1][0], pts[pts.length - 1][1]).commit()
            wait(50)
            out.push(xy(item))
            return out.join(" ")
        }

        function test_1_drag_handler_on_a_turned_target() {
            // 6 down the window is under the threshold; then the target follows one for one.
            compare(drag(live.turned, [[40, 30], [40, 36], [40, 45], [50, 55], [30, 60], [100, 150]]),
                    "20,20 20,35 30,45 10,50 80,140 80,140")
        }

        function test_2_drag_handler_on_a_target_with_a_transform_list() {
            // 12 in the window is 8 in the target: under the threshold. Qt maps the travel
            // through the target's whole transform about its transformOrigin point, so with a
            // list that turns about the corner the target jumps when the drag starts.
            compare(drag(live.tilted, [[85, 50], [85, 56], [85, 62], [85, 80], [70, 95], [150, 60]]),
                    "100,20 100,20 65,70 50,85 130,50 130,50")
        }

        function test_3_axis_limits_in_a_scaled_item() {
            // 22 in the window is 11 in the item; 30 and 2 are the item's own units.
            compare(drag(live.boxed, [[30, 90], [40, 90], [52, 90], [72, 110], [200, 90], [10, 130]]),
                    "0,0 11,0 21,0 30,0 2,0 2,0")
        }

        function test_4_mouse_area_drag_axis_and_limits_in_a_scaled_item() {
            // 20 in the window is 10 in the item: not over the threshold of 10. At 22 it is, and
            // the drag starts from there (drag.smoothed).
            compare(drag(live.slider, [[190, 30], [196, 30], [210, 30], [212, 30], [232, 50], [299, 30], [150, 30]]),
                    "10,0 10,0 10,0 20,0 40,0 4,0 4,0")
        }

        function test_5_mouse_area_drag_threshold_in_a_turned_scaled_item() {
            // The item is upside down at half size: 2 in the window is 4 in the item, against
            // the pointer. Not smoothed: the travel counts from the press.
            compare(drag(live.puck, [[230, 120], [230, 121], [230, 122], [226, 122], [226, 116], [240, 100], [200, 140]]),
                    "20,10 20,10 28,6 28,18 0,20 80,0 80,0")
        }

        function test_6_drag_handler_by_a_finger() {
            compare(touch(live.finger, [[30, 160], [36, 160], [48, 160], [60, 170], [90, 150]]),
                    "0,0 12,0 20,6.7 40,-6.7 40,-6.7")
        }

        function test_7_mouse_area_drag_by_a_finger() {
            compare(touch(live.thumb, [[120, 160], [126, 160], [138, 160], [150, 170], [100, 150]]),
                    "60,0 60,0 68,6.7 34.7,-6.7 34.7,-6.7")
        }
    }
}

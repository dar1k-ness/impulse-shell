import QtQuick
import QtTest

// The values ../transform_live.rs asserts, checked in Qt 6.12 itself. In the tools/testenv image:
//   QT_QPA_PLATFORM=wayland /usr/lib/qt6/bin/qmltestrunner -input crates/shell/tests/transform_live/tst_transform_live.qml
Item {
    width: 300; height: 200

    Live { id: live }

    TestCase {
        name: "transform_live"
        when: windowShown

        function px(img, x, y) {
            var c = img.pixel(x, y)
            return [Math.round(c.r * 255), Math.round(c.g * 255), Math.round(c.b * 255)].join(",")
        }
        function at(pts) {
            var img = grabImage(live)
            return pts.map(p => px(img, p[0], p[1])).join(" ")
        }
        function took() {
            var l = live.log.trim()
            live.log = ""
            return l
        }

        function test_1_binding_and_behavior() {
            compare(live.where(), "10,10 10,50 10,90 10,130")
            compare(at([[15, 15], [55, 25], [65, 35]]), "255,0,0 0,0,0 0,0,0")
            live.p = 20
            // The binding moves its item at once; the Behavior has not moved its own yet.
            compare(live.where(), "50,20 10,50 10,90 10,130")
            // On its way the item is where the Translate says.
            tryVerify(() => live.eased.x > 2 && live.eased.x < 18)
            var x = live.eased.x
            var w = live.where().split(" ")[1].split(",")
            verify(Math.abs(w[0] - (10 + x)) < 0.11, "mid way: " + w + " for " + x)
            tryCompare(live.eased, "x", 20)
            tryCompare(live.eased, "y", 5)
            compare(live.where(), "50,20 30,55 10,90 10,130")
            compare(at([[15, 15], [55, 25], [65, 35], [15, 55], [35, 60], [45, 70], [55, 60]]),
                    "0,0,0 255,0,0 255,0,0 0,0,0 0,255,0 0,255,0 0,0,0")
        }

        function test_3_animation() {
            live.startRun()
            tryCompare(live.run, "running", false)
            live.startLoop()
            tryCompare(live.loop, "running", false)
            compare(live.where(), "50,20 30,55 70,150 10,170")
            // [Scale 2, Translate 60]: 40 wide at 70..110 x 150..170; the other at 10..30 x 170..190.
            compare(at([[15, 95], [75, 155], [105, 165], [115, 160], [15, 135], [15, 175], [25, 185]]),
                    "0,0,0 0,0,255 0,0,255 0,0,0 0,0,0 255,255,0 255,255,0")
        }

        function test_4_hover() {
            mouseMove(live, 290, 5)
            took()
            mouseMove(live, 130, 30)
            compare(took(), "turned-in turned 10,15")
            mouseMove(live, 140, 55)
            compare(took(), "turned 35,5")
            mouseMove(live, 150, 55)
            compare(took(), "turned-out")
            mouseMove(live, 230, 115)
            compare(took(), "scaled-in scaled 5,10")
            mouseMove(live, 275, 125)
            compare(took(), "scaled 27.5,30")
            mouseMove(live, 285, 125)
            compare(took(), "scaled-out")
            mouseMove(live, 210, 125)
            compare(took(), "")
        }

        function test_5_hover_follows_a_moving_item() {
            mouseMove(live, 150, 165)
            compare(took(), "")
            live.slide.x = -60
            tryCompare(live, "log", "slid true ")
            took()
            live.slide.x = 0
            tryCompare(live, "log", "slid false ")
            took()
        }

        function test_6_drag() {
            compare(live.knob.x, 0)
            mousePress(live, 110, 30)
            mouseMove(live, 110, 40)
            // The drag starts with the first move: the knob follows from there.
            compare(live.knob.x, 0)
            mouseMove(live, 115, 62)
            compare(live.knob.x, 22)
            mouseMove(live, 115, 150)
            compare(live.knob.x, 80)
            mouseRelease(live, 115, 150)
            compare(live.knob.x, 80)
        }
    }
}

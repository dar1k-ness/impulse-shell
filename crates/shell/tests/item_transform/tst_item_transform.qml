import QtQuick
import QtTest

// The values ../item_transform.rs asserts, checked in Qt 6.12 itself. In the tools/testenv image:
//   QT_QPA_PLATFORM=wayland /usr/lib/qt6/bin/qmltestrunner -input crates/shell/tests/item_transform/tst_item_transform.qml
Item {
    width: 400; height: 500

    Paint { id: paint }
    Input { id: input; y: 300 }

    TestCase {
        name: "item_transform"
        when: windowShown

        function px(img, x, y) {
            var c = img.pixel(x, y)
            return [Math.round(c.r * 255), Math.round(c.g * 255), Math.round(c.b * 255)].join(",")
        }
        function at(img, pts) {
            return pts.map(p => px(img, p[0], p[1])).join(" ")
        }
        function click(x, y) {
            input.log = ""
            mouseClick(input, x, y)
            return input.log
        }

        function test_paint() {
            var img = grabImage(paint)
            compare(at(img, [[45, 20], [55, 30], [20, 20], [35, 20], [65, 20], [50, 12]]), "255,0,0 255,0,0 0,0,0 0,0,0 0,0,0 0,0,0", "translate")
            compare(at(img, [[93, 20], [127, 20], [110, 12], [110, 28], [87, 20], [133, 20]]), "0,255,0 0,255,0 0,0,0 0,0,0 0,0,0 0,0,0", "scale")
            compare(at(img, [[195, 15], [195, 45], [220, 15], [205, 30], [185, 30], [195, 55]]), "0,0,255 0,0,255 0,0,0 0,0,0 0,0,0 0,0,0", "rotation")
            compare(at(img, [[313, 13], [347, 47], [305, 20], [355, 20]]), "255,0,255 255,0,255 0,0,0 0,0,0", "own scale first")
            compare(at(img, [[53, 85], [87, 85], [45, 85], [95, 85]]), "255,255,0 255,255,0 0,0,0 0,0,0", "scaled, then moved")
            compare(at(img, [[83, 105], [117, 105], [75, 105], [125, 105]]), "255,255,0 255,255,0 0,0,0 0,0,0", "moved, then scaled")
            compare(at(img, [[213, 103], [247, 117], [205, 110], [255, 110], [230, 95], [230, 125]]), "0,255,255 0,255,255 0,0,0 0,0,0 0,0,0 0,0,0", "matrix")
            compare(at(img, [[323, 85], [337, 85], [315, 85], [345, 85]]), "128,128,128 128,128,128 0,0,0 0,0,0", "nested")
            compare(at(img, [[73, 153], [107, 187], [65, 170], [115, 170], [40, 170], [90, 145], [90, 195]]), "255,255,255 255,255,255 0,0,0 0,0,0 0,0,0 0,0,0 0,0,0", "clip moved")
            compare(at(img, [[183, 153], [197, 187], [175, 170], [205, 170], [190, 145], [190, 195], [230, 160]]), "255,255,255 255,255,255 0,0,0 0,0,0 0,0,0 0,0,0 0,0,0", "clip turned")
            compare(at(img, [[333, 153], [337, 167], [345, 160], [325, 160], [335, 175]]), "255,255,255 255,255,255 0,0,0 0,0,0 0,0,0", "parent clip")
            compare(at(img, [[70, 245], [50, 245], [90, 245], [25, 245], [115, 245], [38, 245], [100, 245]]), "255,128,0 255,128,0 255,128,0 0,0,0 0,0,0 0,0,0 0,0,0", "tilted")
            compare(at(img, [[70, 215], [70, 275], [46, 224], [92, 222]]), "0,0,0 0,0,0 255,128,0 255,128,0", "tilted, the edges")
            compare(at(img, [[115, 20], [125, 30], [45, 20], [55, 30]]), "0,255,0 0,0,0 255,0,0 255,0,0")
            paint.shift.x = 100
            img = grabImage(paint)
            compare(at(img, [[115, 20], [125, 30], [45, 20], [55, 30]]), "0,255,0 255,0,0 0,0,0 0,0,0", "moved on")
        }

        function clicks(pts) {
            return pts.map(p => click(p[0], p[1]).trim()).join("; ")
        }

        function test_input() {
            compare(clicks([[50, 20], [65, 20]]), "moved 10,10; back", "Translate")
            compare(clicks([[93, 20], [127, 24], [110, 12], [110, 28]]), "scaled 1.5,10; scaled 18.5,18; back; back", "Scale")
            compare(clicks([[195, 40], [199, 11], [220, 15], [195, 55]]), "turned 30,5; turned 1,1; back; back", "Rotation")
            compare(clicks([[265, 85], [231, 81], [225, 85], [275, 85]]), "list 17.5,5; list 0.5,1; back; back", "[Scale, Translate]")
            compare(clicks([[70, 145], [50, 145], [90, 145], [25, 145], [115, 145]]),
                    "tilted 50,25; tilted 11.3,25; tilted 91.4,25; back; back", "Rotation about the y axis")
            compare(clicks([[46, 124], [92, 122]]), "tilted 3.9,4.8; tilted 95.7,1.1", "its corners")
            compare(clicks([[90, 80], [72, 62], [125, 80], [40, 80], [65, 80]]),
                    "clipped 20,20; clipped 2,2; back; back; back", "a moved clip")
            // Scaled to nothing: clicked on its size at its parent's origin, and the position
            // it reports is the scene's (`input` is at y 300 here).
            compare(clicks([[325, 145], [300, 145], [20, 20], [41, 29]]), "back; back; flat 20,320; flat 41,329", "no inverse")
        }
    }
}

import QtQuick
import QtQuick.Controls

// The Flickable a ScrollView makes is `pixelAligned`: what it shows, and so the position of its
// bar, is its content at a whole pixel (QQuickFlickableVisibleArea::updateVisible). One declared
// in the view (`b`) and a bare one (`c`) are not, their bars stand where the content is.
Item {
    id: root
    width: 300; height: 330

    function part() { a.contentItem.contentY = 10.4; bf.contentY = 10.4; c.contentY = 10.4; }
    function half() { a.contentItem.contentY = 10.5; bf.contentY = 10.5; c.contentY = 10.5; }
    function more() { a.contentItem.contentY = 11.6; bf.contentY = 11.6; c.contentY = 11.6; }

    function snap() {
        const r = v => Math.round(v * 1000000) / 1000000;
        const one = (n, f, o) => n + ": aligned " + f.pixelAligned + " at " + r(f.contentY) + " of " + r(f.contentHeight)
            + " position " + r(o.position) + " size " + r(o.size)
            + " handle " + r(o.contentItem.y) + " " + r(o.contentItem.height);
        return one("a", a.contentItem, a.ScrollBar.vertical) + "\n"
            + one("b", bf, b.ScrollBar.vertical) + "\n"
            + one("c", c, c.ScrollBar.vertical);
    }

    ScrollView {
        id: a
        x: 0; y: 0; width: 100; height: 100
        Rectangle { implicitWidth: 80; implicitHeight: 370; color: "teal" }
    }
    ScrollView {
        id: b
        x: 0; y: 110; width: 100; height: 100
        Flickable {
            id: bf
            contentWidth: 80; contentHeight: 370
            Rectangle { width: 80; height: 370; color: "teal" }
        }
    }
    Flickable {
        id: c
        x: 0; y: 220; width: 100; height: 100
        contentWidth: 80; contentHeight: 370
        ScrollBar.vertical: ScrollBar {}
        Rectangle { width: 80; height: 370; color: "teal" }
    }
}

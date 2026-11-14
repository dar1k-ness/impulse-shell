// The size of a view's content item (QQuickFlickable::geometryChange, setContentWidth and
// setContentHeight): the view's size less its margins on an axis whose content size is not
// set, once the view's size on that axis has changed; 0 before that. An item view sets the
// content size on the axis its items flow along (QQuickItemViewPrivate::updateViewport), 0
// with nothing in it. Each row: the view's width and height, its content item's width and
// height; before and after the sizes change.
// qt: 80 100 80 0; 80 100 77 0; 80 100 80 0; 80 100 0 93; 80 100 77 84; 0 0 0 0; 0 0 0 0; 80 100 77 0 | 80 100 80 0; 80 50 77 0; 80 100 80 0; 40 100 0 93; 80 50 77 34; 0 20 0 4; 0 20 0 0; 80 100 77 0
import QtQuick

Item {
    id: root
    width: 300
    height: 400
    property string result

    function s(v) {
        return [v.width, v.height, v.contentItem.width, v.contentItem.height].join(" ");
    }
    function rows() {
        return [s(a), s(b), s(c), s(d), s(e), s(f), s(g), s(h)].join("; ");
    }

    ListView { id: a; width: 80; height: 100 }
    ListView { id: b; width: 80; height: 100; topMargin: 16; bottomMargin: 16; leftMargin: 3 }
    ListView { id: c; width: 80; height: 100; topMargin: 16; model: 0; delegate: Item { height: 20 } }
    ListView { id: d; width: 80; height: 100; orientation: ListView.Horizontal; leftMargin: 5; rightMargin: 5; topMargin: 7 }
    Flickable { id: e; width: 80; height: 100; topMargin: 16; leftMargin: 3 }
    Flickable { id: f; topMargin: 16; leftMargin: 3 }
    ListView { id: g; topMargin: 16; leftMargin: 3 }
    GridView { id: h; width: 80; height: 100; topMargin: 16; leftMargin: 3 }

    Component.onCompleted: {
        var first = rows();
        b.height = 50;
        e.height = 50;
        f.height = 20;
        g.height = 20;
        d.width = 40;
        result = first + " | " + rows();
    }
}

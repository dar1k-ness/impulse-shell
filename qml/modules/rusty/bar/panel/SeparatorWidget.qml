import QtQuick

// the original's separators next to the workspaces (BarContent / VerticalBarContent): 1 px Rectangles in a
// Row / Column, where their Layout.fillHeight / fillWidth does nothing. Left with no height (no
// width on the vertical bar), the positioner skips them: with `bar.borderless` on too they take
// no space and draw no line. The entry only ends the group before it and starts the next.
Item {
    readonly property bool shown: false
    visible: false
}

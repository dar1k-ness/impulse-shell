import QtQuick
import Rustshell
import ri.modules.common
import ri.modules.waffle.looks

WPopupToolTip {
    anchorEdges: Config.options.waffles.bar.bottom ? Edges.Top : Edges.Bottom
}

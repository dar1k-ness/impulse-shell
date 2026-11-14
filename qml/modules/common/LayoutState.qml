pragma Singleton
pragma ComponentBehavior: Bound
import QtQuick
import Rustshell

/**
 * The container kinds ("panel", "desktop", "sidebar") the widget layout
 * (~/.config/rusty-impulse/layout.toml) defines. rusty-impulse fills containerKinds from its layout
 * model; with no layout the surfaces render their built-in the original content.
 */
Singleton {
    id: root
    property list<string> containerKinds: []
    // connector -> edge of that monitor's panel; "" -> the primary monitor's
    property var panelEdges: ({})

    function hasContainer(kind: string): bool {
        return root.containerKinds.includes(kind);
    }

    // The edge ("top", "bottom", "left", "right") of the layout's panel on a screen, "" with none.
    function panelEdge(screenName: string): string {
        return root.panelEdges[screenName] ?? root.panelEdges[""] ?? "";
    }
}

import QtQuick
import QtQuick.Layouts

// One zone of a panel (start, center or end): its runs in layout order along the panel.
GridLayout {
    id: root
    property var runs: []
    property bool vertical: false
    property string edge: "top"
    property bool zoneHovered: false
    property int shortened: 0
    // Where free space goes when no run stretches: after the runs (start zone), before them
    // (end zone), or nowhere (center zone, sized to its runs).
    property bool fillBefore: false
    property bool fillAfter: false
    // A stretching run takes the free space only while it shows: the original hides the active window
    // title on a vertical or shortened bar, and the rest of the zone keeps its place. Only runs
    // are asked for `visible`: the fillers' visibility follows this property.
    readonly property bool stretches: {
        for (let i = 0; i < children.length; i++) {
            const run = children[i];
            if (run.modelData?.stretch && run.visible)
                return true;
        }
        return false;
    }
    property bool center: false
    property real spacing: 4
    // The zone, for its log line ("DP-1 end").
    property string label: ""
    // The length the zone's widgets may take along the panel.
    property real available: vertical ? height : width
    // The zone's widgets at their least lengths, from the bar's edge in (the end zone from its
    // last widget): those that fit show; from the first that does not, the rest, nearest the
    // bar's middle, wait hidden for room, where a full Qt layout would lay them out past its end.
    // A widget that may give way to nothing (the spacer) costs only the spacing beside the
    // free space, which the zone keeps either way, and is never named.
    readonly property var fit: {
        const runs = [];
        for (let i = 0; i < children.length; i++) {
            if (children[i].modelData !== undefined)
                runs.push(children[i]);
        }
        let used = fillBefore || fillAfter ? spacing : 0;
        // What each widget here costs the zone besides spacing: what edit mode charges it.
        const lengths = {};
        for (let k = 0; k < runs.length; k++) {
            for (let j = 0; j < runs[k].count; j++) {
                const host = runs[k].host(j);
                if (host?.item)
                    lengths[host.entry.id] = (host.item.shown ?? true) ? host.minimumLength + (vertical ? host.leadingSpace : 0) : 0;
            }
        }
        // Not laid out yet: nothing to judge.
        if (available <= 0)
            return { used: used, cut: -1, hidden: "", lengths: lengths };
        let shownRuns = 0;
        let cut = -1;
        const hidden = [];
        for (let k = 0; k < runs.length; k++) {
            const run = runs[fillBefore ? runs.length - 1 - k : k];
            const n = run.count;
            let shown = 0;
            for (let j = 0; j < n; j++) {
                const at = fillBefore ? n - 1 - j : j;
                const host = run.host(at);
                if (!host?.item || !(host.item.shown ?? true) || host.minimumLength <= 0)
                    continue;
                if (cut < 0) {
                    // the original's center side groups keep their fixed length, whatever they hold.
                    const fixed = center && !vertical && run.modelData.stretch;
                    const cost = fixed
                        ? (shown > 0 ? 0 : run.centerSideLength + (shownRuns > 0 ? spacing : 0))
                        : host.minimumLength + (vertical ? host.leadingSpace : 0) + (shown > 0 ? run.spacing : run.padding * 2 + (shownRuns > 0 ? spacing : 0));
                    // The widget at the bar's edge (the original's sidebar buttons) always shows: the
                    // center zone keeps its length free (edgeLength).
                    if (used + cost <= available || (shownRuns === 0 && shown === 0)) {
                        used += cost;
                        shown++;
                        continue;
                    }
                    cut = run.modelData.first + at;
                }
                hidden.push(host.entry.id);
            }
            if (shown > 0)
                shownRuns++;
        }
        return { used: used, cut: cut, hidden: hidden.join(", "), lengths: lengths };
    }
    readonly property var lengths: fit.lengths
    // What the zone's widget nearest the bar's edge takes along it, with what the zone keeps
    // beside it: the length no other zone may take from this one.
    readonly property real edgeLength: {
        const runs = [];
        for (let i = 0; i < children.length; i++) {
            if (children[i].modelData !== undefined)
                runs.push(children[i]);
        }
        for (let k = 0; k < runs.length; k++) {
            const run = runs[fillBefore ? runs.length - 1 - k : k];
            for (let j = 0; j < run.count; j++) {
                const host = run.host(fillBefore ? run.count - 1 - j : j);
                if (host?.item && (host.item.shown ?? true) && host.minimumLength > 0)
                    return (fillBefore || fillAfter ? spacing : 0) + host.minimumLength + (vertical ? host.leadingSpace : 0) + run.padding * 2;
            }
        }
        return 0;
    }
    readonly property string hidden: fit.hidden
    onHiddenChanged: {
        if (hidden !== "")
            console.info("[Panel] " + label + " zone has no room for " + hidden + ": hidden");
        else
            console.info("[Panel] " + label + " zone shows all its widgets");
    }
    // The length the zone has left; none once it hides a widget.
    readonly property real freeLength: fit.cut >= 0 ? 0 : Math.max(0, available - fit.used)
    // What one more widget costs besides its own length, whichever run it lands in: the
    // spacing and group padding of a new pill run (PanelRun), or the spacing in a run it joins.
    readonly property real overhead: Math.max((fit.used > 0 ? spacing : 0) + 2 * (vertical ? 8 : 5), vertical ? 12 : 4)
    // The length one more widget may take here without any widget giving up its own.
    readonly property real room: Math.max(0, freeLength - overhead)

    columns: vertical ? 1 : -1
    columnSpacing: spacing
    rowSpacing: spacing

    Item {
        visible: root.fillBefore && !root.stretches
        Layout.fillWidth: !root.vertical
        Layout.fillHeight: root.vertical
    }
    Repeater {
        model: root.runs
        delegate: PanelRun {
            vertical: root.vertical
            edge: root.edge
            zoneHovered: root.zoneHovered
            shortened: root.shortened
            center: root.center
            cut: root.fit.cut
            keepAfter: root.fillBefore
        }
    }
    Item {
        visible: root.fillAfter && !root.stretches
        Layout.fillWidth: !root.vertical
        Layout.fillHeight: root.vertical
    }
}

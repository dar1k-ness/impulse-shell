//@ pragma UseQApplication
//@ pragma Env QS_NO_RELOAD_POPUP=1
//@ pragma Env QT_QUICK_CONTROLS_STYLE=Basic
//@ pragma Env QT_QUICK_FLICKABLE_WHEEL_DECELERATION=10000
// Interaction harness for tests/interact.rs: one instance of every interactive control type
// the tree uses, at fixed positions, logging its signals and resulting state as `IACT ...` lines.
// The test copies the tree next to this file (so `qs.*` imports resolve) and runs it under both
// `qs -p` and `rusty-impulse -p`, driving identical pointer/keyboard input.
import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Wayland
import qs.modules.common
import qs.modules.common.widgets
import qs.modules.waffle.looks

ShellRoot {
    PanelWindow {
        id: win
        anchors.top: true
        anchors.left: true
        implicitWidth: 1200
        implicitHeight: 760
        exclusionMode: ExclusionMode.Ignore
        WlrLayershell.namespace: "quickshell:iact"
        WlrLayershell.keyboardFocus: WlrKeyboardFocus.OnDemand
        color: "#202020"

        function r(v) {
            return typeof v === "number" ? Math.round(v * 100) / 100 : v;
        }
        function lg(name, what, v) {
            console.log("IACT " + name + " " + what + (v === undefined ? "" : " " + r(v)));
        }
        function geom(name, it) {
            if (!it)
                return;
            const p = it.mapToItem(null, 0, 0);
            console.log("IACT geom " + name + " " + Math.round(p.x) + " " + Math.round(p.y) + " " + Math.round(it.width) + " " + Math.round(it.height));
        }

        // The test's input clock, outside the trace: Qt (QWaylandEventCompressionPrivate) and
        // rusty-impulse both merge pointer frames that queued up behind each other, so the test sends
        // the next one only once two frames were drawn after the previous, i.e. after the shell's
        // event loop has handled it on its own.
        FrameAnimation {
            running: true
            onTriggered: console.log("ITICK")
        }

        // `ready` once the original's Config loaded and the pointer (parked over the window by the test)
        // entered the mapped window: from then on input reaches it.
        MouseArea {
            id: enterProbe
            anchors.fill: parent
            z: -1
            hoverEnabled: true
            acceptedButtons: Qt.NoButton
        }
        readonly property bool live: Config.ready && enterProbe.containsMouse
        property bool started: false
        onLiveChanged: {
            if (!live || started)
                return;
            started = true;
            const g = [["btn", btn], ["combo", combo], ["slider", slider], ["sw", sw], ["radio1", radio1], ["radio2", radio2], ["spin", spin], ["spinUp", spin.up.indicator], ["spinDown", spin.down.indicator], ["tf", tf], ["ta", ta], ["tabs", tabs], ["tab1", tab1], ["tab2", tab2], ["list", list], ["flick", flick], ["ma", ma], ["drg", drg], ["wh", wh], ["keys", keys], ["menuBtn", menuBtn], ["tipBtn", tipBtn], ["swipe", swipe], ["popupBtn", popupBtn], ["toggleBtn", toggleBtn], ["dragTarget", dragTarget], ["swipeTo2", swipeTo2], ["swipeTo0", swipeTo0], ["dblBtn", dblBtn], ["grp", grp]];
            for (const [n, it] of g)
                win.geom(n, it);
            console.log("IACT ready");
        }

        RippleButton {
            id: btn
            x: 20; y: 20; width: 160; height: 40
            buttonText: "Button"
            onClicked: win.lg("btn", "clicked")
            onPressed: win.lg("btn", "pressed")
            onReleased: win.lg("btn", "released")
            onPressAndHold: win.lg("btn", "pressAndHold")
            onHoveredChanged: win.lg("btn", "hovered", hovered)
            onDownChanged: win.lg("btn", "down", down)
        }

        RippleButton {
            id: toggleBtn
            x: 20; y: 70; width: 160; height: 40
            buttonText: "Toggle"
            toggled: checkedState
            property bool checkedState: false
            onClicked: { checkedState = !checkedState; win.lg("toggleBtn", "clicked", checkedState); }
            altAction: () => win.lg("toggleBtn", "altAction")
            middleClickAction: () => win.lg("toggleBtn", "middleClick")
        }

        StyledComboBox {
            id: combo
            x: 200; y: 20; width: 200
            model: ["Alpha", "Beta", "Gamma"]
            onActivated: index => win.lg("combo", "activated", index)
            onCurrentIndexChanged: win.lg("combo", "currentIndex", currentIndex)
            onHighlightedIndexChanged: win.lg("combo", "highlightedIndex", highlightedIndex)
            onDisplayTextChanged: win.lg("combo", "displayText", displayText)
            onPressedChanged: win.lg("combo", "pressed", pressed)
            Connections {
                target: combo.popup
                function onVisibleChanged() { win.lg("combo", "popupVisible", combo.popup.visible); }
                function onOpened() { win.lg("combo", "opened"); win.geom("comboPopup", combo.popup.contentItem); }
                function onClosed() { win.lg("combo", "closed"); }
            }
        }

        StyledSlider {
            id: slider
            x: 20; y: 240; width: 300
            from: 0; to: 1
            value: 0.25
            onMoved: win.lg("slider", "moved")
            onPressedChanged: win.lg("slider", "pressed", pressed + " " + win.r(value))
        }

        StyledSwitch {
            id: sw
            x: 450; y: 20
            onToggled: win.lg("sw", "toggled", checked)
            onClicked: win.lg("sw", "clicked")
            onCheckedChanged: win.lg("sw", "checked", checked)
        }

        StyledRadioButton {
            id: radio1
            x: 450; y: 80; width: 150
            text: "One"
            checked: true
            onCheckedChanged: win.lg("radio1", "checked", checked)
            onClicked: win.lg("radio1", "clicked")
        }
        StyledRadioButton {
            id: radio2
            x: 450; y: 120; width: 150
            text: "Two"
            onCheckedChanged: win.lg("radio2", "checked", checked)
            onClicked: win.lg("radio2", "clicked")
        }

        StyledSpinBox {
            id: spin
            x: 450; y: 170
            from: 0; to: 10; value: 5
            onValueModified: if (!spin.repeated()) win.lg("spin", "valueModified", value)
            onValueChanged: if (!spin.repeated()) win.lg("spin", "value", value)

            // Outside the trace: a click on a button of the spin box that Qt did not step for.
            // QQuickSpinBoxPrivate::handleRelease steps unless its repeat timer runs, and that
            // one starts 300 ms after the press by the wall clock. Qt handles the release of a
            // click from a 0 ms timer (QWaylandEventCompressionPrivate holds its frame back),
            // so a GUI thread kept off the CPU for 300 ms after the press finds both timers
            // due, starts the repeat first and drops the click. The release sets `pressed`
            // after it stepped: a value still the one of the press, with room to step, is
            // that case and no other. The test then clicks again.
            //
            // A release Qt handles 500 ms or more after the press is a hold for it: the repeat
            // steps 400 ms after the press and then every 100 ms until the release. A repeated
            // step prints what a click's does, so the first one is the click's; the ones after
            // it, told by a value further than one step from the press's while the button is
            // down, stay out of the trace (`IREPEATED`) and are taken back once Qt's release
            // handler returned: a value it finds changed is a valueModified of its own.
            property int pressedAt
            property int kept
            property bool buttonDown
            property bool takingBack
            function repeated() {
                return takingBack || buttonDown && Math.abs(value - pressedAt) > 1;
            }
            function takeBack() {
                takingBack = true;
                value = kept;
                takingBack = false;
            }
            function held(button, room) {
                if (button.pressed) {
                    pressedAt = value;
                    buttonDown = true;
                    return;
                }
                if (room && value === pressedAt) {
                    console.log("IHELD");
                } else if (repeated()) {
                    console.log("IREPEATED " + Math.abs(value - pressedAt));
                    kept = pressedAt + (value > pressedAt ? 1 : -1);
                    Qt.callLater(takeBack);
                }
                buttonDown = false;
            }
            Connections {
                target: spin.up
                function onPressedChanged() {
                    spin.held(spin.up, spin.value < spin.to);
                }
            }
            Connections {
                target: spin.down
                function onPressedChanged() {
                    spin.held(spin.down, spin.value > spin.from);
                }
            }
        }

        MaterialTextField {
            id: tf
            x: 20; y: 300; width: 300
            placeholderText: "Type"
            onAccepted: win.lg("tf", "accepted", text)
            onTextEdited: win.lg("tf", "textEdited", text)
            onTextChanged: win.lg("tf", "text", text)
            onActiveFocusChanged: win.lg("tf", "activeFocus", activeFocus)
        }

        StyledTextArea {
            id: ta
            x: 20; y: 370; width: 300; height: 80
            onTextChanged: win.lg("ta", "text", text.replace(/\n/g, "\\n"))
            onActiveFocusChanged: win.lg("ta", "activeFocus", activeFocus)
        }

        SecondaryTabBar {
            id: tabs
            x: 700; y: 20; width: 400
            onCurrentIndexChanged: win.lg("tabs", "currentIndex", currentIndex)
            SecondaryTabButton { id: tab0; buttonText: "A"; buttonIcon: "home" }
            SecondaryTabButton { id: tab1; buttonText: "B"; buttonIcon: "star" }
            SecondaryTabButton { id: tab2; buttonText: "C"; buttonIcon: "settings" }
        }

        StyledListView {
            id: list
            x: 700; y: 100; width: 200; height: 200
            clip: true
            model: 30
            // Wheel flicks start from the timeline value of the last frame, and a shell may handle
            // a notch after the previous flick settled: log wheel movements coalesced, without
            // positions, ending once the pointer moved after the notches (the test's end of
            // the burst, listWheelProbe); drag movements with the distance moved.
            property real moveFrom: 0
            property bool dragMove: false
            property bool shownMoving: false
            property bool wheelOpen: false
            function settle() {
                if (shownMoving && !moving && !dragMove && !wheelOpen) {
                    shownMoving = false
                    win.lg("list", "moving", "false")
                }
            }
            onMovingChanged: {
                if (moving) {
                    if (!shownMoving) {
                        shownMoving = true
                        moveFrom = contentY
                        dragMove = dragging
                        win.lg("list", "moving", "true" + (dragMove ? " 0" : ""))
                    }
                } else if (dragMove) {
                    shownMoving = false
                    win.lg("list", "moving", "false " + Math.round(contentY - moveFrom))
                } else {
                    settle()
                }
            }
            onFlickStarted: if (!shownMoving || dragMove) win.lg("list", "flickStarted")
            delegate: Rectangle {
                required property int index
                width: 200; height: 40
                color: index % 2 ? "#333" : "#444"
                MouseArea {
                    anchors.fill: parent
                    onClicked: win.lg("list", "delegateClicked", parent.index)
                }
            }
        }

        // Sees list's wheel notches (passing them on) and the pointer motion after them.
        MouseArea {
            id: listWheelProbe
            x: list.x; y: list.y; width: list.width - 20; height: list.height
            hoverEnabled: true
            acceptedButtons: Qt.NoButton
            property point at
            onWheel: wheel => {
                at = Qt.point(wheel.x, wheel.y);
                list.wheelOpen = true;
                wheel.accepted = false;
            }
            onPositionChanged: mouse => {
                if (list.wheelOpen && (mouse.x !== at.x || mouse.y !== at.y)) {
                    list.wheelOpen = false;
                    list.settle();
                }
            }
        }

        StyledFlickable {
            id: flick
            x: 950; y: 100; width: 200; height: 200
            clip: true
            contentHeight: 1000
            // Wheel flicks start from the timeline value of the last frame, and a shell may handle
            // a notch after the previous flick settled: log wheel movements coalesced, without
            // positions, ending once the pointer moved after the notches (the test's end of
            // the burst, flickWheelProbe); drag movements with the distance moved.
            property real moveFrom: 0
            property bool dragMove: false
            property bool shownMoving: false
            property bool wheelOpen: false
            function settle() {
                if (shownMoving && !moving && !dragMove && !wheelOpen) {
                    shownMoving = false
                    win.lg("flick", "moving", "false")
                }
            }
            onMovingChanged: {
                if (moving) {
                    if (!shownMoving) {
                        shownMoving = true
                        moveFrom = contentY
                        dragMove = dragging
                        win.lg("flick", "moving", "true" + (dragMove ? " 0" : ""))
                    }
                } else if (dragMove) {
                    shownMoving = false
                    win.lg("flick", "moving", "false " + Math.round(contentY - moveFrom))
                } else {
                    settle()
                }
            }
            onFlickStarted: if (!shownMoving || dragMove) win.lg("flick", "flickStarted")
            ScrollBar.vertical: StyledScrollBar {
                onPressedChanged: win.lg("flick", "barPressed", pressed + " " + Math.round(flick.contentY))
            }
            Rectangle { width: 200; height: 1000; color: "#355" }
        }

        // Sees flick's wheel notches (passing them on) and the pointer motion after them.
        MouseArea {
            id: flickWheelProbe
            x: flick.x; y: flick.y; width: flick.width - 20; height: flick.height
            hoverEnabled: true
            acceptedButtons: Qt.NoButton
            property point at
            onWheel: wheel => {
                at = Qt.point(wheel.x, wheel.y);
                flick.wheelOpen = true;
                wheel.accepted = false;
            }
            onPositionChanged: mouse => {
                if (flick.wheelOpen && (mouse.x !== at.x || mouse.y !== at.y)) {
                    flick.wheelOpen = false;
                    flick.settle();
                }
            }
        }

        Rectangle {
            x: 20; y: 480; width: 200; height: 100
            color: "#533"
            MouseArea {
                id: ma
                anchors.fill: parent
                hoverEnabled: true
                acceptedButtons: Qt.LeftButton | Qt.RightButton | Qt.MiddleButton
                onEntered: win.lg("ma", "entered")
                onExited: win.lg("ma", "exited")
                onPressed: mouse => win.lg("ma", "pressed", mouse.button)
                onReleased: mouse => win.lg("ma", "released", mouse.button)
                onClicked: mouse => win.lg("ma", "clicked", mouse.button + " " + Math.round(mouse.x) + " " + Math.round(mouse.y))
                onDoubleClicked: mouse => win.lg("ma", "doubleClicked", mouse.button)
                onPressAndHold: mouse => win.lg("ma", "pressAndHold", mouse.button)
                onWheel: wheel => win.lg("ma", "wheel", wheel.angleDelta.y)
                onContainsMouseChanged: win.lg("ma", "containsMouse", containsMouse)
            }
        }

        Rectangle {
            id: drg
            x: 250; y: 480; width: 80; height: 80
            color: "#353"
            DragHandler {
                id: dh
                xAxis.enabled: true
                yAxis.enabled: false
                target: null
                onActiveChanged: win.lg("drg", "active", active + " " + Math.round(translation.x))
            }
        }

        Rectangle {
            id: wh
            x: 360; y: 480; width: 100; height: 100
            color: "#335"
            WheelHandler {
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                onWheel: event => win.lg("wh", "wheel", event.angleDelta.y)
            }
        }

        Rectangle {
            id: keys
            x: 480; y: 480; width: 100; height: 100
            color: activeFocus ? "#555" : "#333"
            focus: false
            onActiveFocusChanged: win.lg("keys", "activeFocus", activeFocus)
            Keys.onPressed: event => win.lg("keys", "pressed", event.key + " " + event.text)
            Keys.onReleased: event => { if (!event.isAutoRepeat) win.lg("keys", "released", event.key); }
            Keys.onEscapePressed: win.lg("keys", "escape")
            MouseArea {
                anchors.fill: parent
                onClicked: keys.forceActiveFocus()
            }
        }

        Rectangle {
            id: dragTarget
            x: 600; y: 480; width: 60; height: 60
            color: "#553"
            MouseArea {
                anchors.fill: parent
                drag.target: dragTarget
                drag.axis: Drag.XAxis
                drag.minimumX: 600
                drag.maximumX: 900
                onReleased: win.lg("dragTarget", "released", dragTarget.x)
                onClicked: win.lg("dragTarget", "clicked")
                drag.onActiveChanged: win.lg("dragTarget", "dragActive", drag.active)
            }
        }

        // An overlay widget (StyledOverlayWidget): a hover-enabled MouseArea that its own
        // DragHandler moves, with a cursor that depends on where mouseX/mouseY say the pointer is.
        MouseArea {
            id: ovw
            x: 760; y: 560; width: 100; height: 60
            hoverEnabled: true
            readonly property bool edge: !(8 < mouseX && mouseX < width - 8 && 8 < mouseY && mouseY < height - 8)
            cursorShape: ovwDrag.active ? Qt.ArrowCursor : edge ? Qt.SizeFDiagCursor : Qt.ArrowCursor
            onEdgeChanged: win.lg("ovw", "edge", edge)
            Rectangle { anchors.fill: parent; color: "#535" }
            DragHandler {
                id: ovwDrag
                onActiveChanged: {
                    win.lg("ovw", "active", active + " at " + ovw.x + " " + ovw.y);
                    if (!active)
                        win.lg("ovw", "pointer", ovw.mouseX + " " + ovw.mouseY + " cursor " + ovw.cursorShape);
                }
            }
        }

        RippleButton {
            id: menuBtn
            x: 20; y: 620; width: 160; height: 40
            buttonText: "Menu"
            onActiveFocusChanged: win.lg("menuBtn", "activeFocus", activeFocus)
            onClicked: menu.popup(menuBtn, 0, menuBtn.height)
            Menu {
                id: menu
                onOpened: { win.lg("menu", "opened"); win.geom("menuContent", menu.contentItem); }
                onClosed: win.lg("menu", "closed")
                MenuItem { text: "First"; onTriggered: win.lg("menu", "triggered", "First") }
                MenuItem { text: "Second"; onTriggered: win.lg("menu", "triggered", "Second") }
            }
        }

        RippleButton {
            id: tipBtn
            x: 200; y: 620; width: 160; height: 40
            buttonText: "Tip"
            StyledToolTip {
                id: tip
                text: "Tooltip"
                onVisibleChanged: win.lg("tip", "visible", visible)
            }
        }

        RippleButton {
            id: popupBtn
            x: 380; y: 620; width: 160; height: 40
            buttonText: "Popup"
            onClicked: plainPopup.open()
            Popup {
                id: plainPopup
                y: -110
                width: 160; height: 100
                onOpened: { win.lg("popup", "opened"); win.geom("popup", plainPopup.contentItem); }
                onClosed: win.lg("popup", "closed")
                MouseArea {
                    anchors.fill: parent
                    onClicked: win.lg("popup", "innerClicked")
                }
            }
        }

        RippleButton {
            id: wmenuBtn
            x: 560; y: 620; width: 160; height: 40
            buttonText: "WMenu"
            onClicked: wmenu.popup()
            WMenu {
                id: wmenu
                downDirection: true
                onAboutToShow: win.lg("wmenu", "aboutToShow")
                onOpened: win.lg("wmenu", "opened")
                onClosed: win.lg("wmenu", "closed")
                WMenuItem { id: wmenuFirst; icon.name: "pin"; text: "Pin to Start"; onTriggered: win.lg("wmenu", "triggered", "First") }
                WMenuItem { visible: false; text: "Hidden"; onTriggered: win.lg("wmenu", "triggered", "Hidden") }
                WMenuItem { id: wmenuThird; text: "Move to front"; onTriggered: { win.lg("wmenu", "triggered", "Third"); win.geom("wmenuFirst", wmenuFirst); win.geom("wmenuThird", wmenuThird); } }
            }
        }

        SwipeView {
            id: swipe
            x: 950; y: 340; width: 200; height: 120
            clip: true
            onCurrentIndexChanged: {
                win.lg("swipe", "currentIndex", currentIndex);
                if (!win.swipeByCode)
                    swipeSettle.restart();
            }
            Rectangle { id: swipePage0; color: "#a33" }
            Rectangle { color: "#3a3" }
            Rectangle { color: "#33a" }
        }

        // A code-driven SwipeView currentIndex change: the Basic ListView contentItem moves its
        // highlight (highlightMoveDuration 250) and the view follows it to the new page. Sampled
        // every frame until the view is at the new page: whether a frame showed it between the
        // pages, then where it came to rest. At rest the offset is the page's own to the last
        // bit; the frame before is a fraction of a pixel short of it and still moving.
        function swipeOffset() {
            return -swipePage0.mapToItem(swipe, 0, 0).x;
        }
        property bool swipeByCode: false
        function swipeGo(i) {
            swipeTrack.from = swipeOffset();
            swipeTrack.midway = false;
            swipeByCode = true;
            swipe.currentIndex = i;
            swipeByCode = false;
            win.lg("swipeCode", "offset", swipeOffset());
            swipeTrack.restart();
        }
        FrameAnimation {
            id: swipeTrack
            property real from
            property bool midway
            onTriggered: {
                const o = win.swipeOffset(), to = swipe.currentIndex * swipe.width;
                if (Math.min(from, to) < o && o < Math.max(from, to))
                    midway = true;
                if (o === to) {
                    stop();
                    win.lg("swipeCode", "midway", midway);
                    win.lg("swipeCode", "rest", Math.round(o));
                }
            }
        }
        // A swipe by drag: the page it settles on once the view stopped following the pointer
        // and its contentItem (Qt's is a ListView, which picks currentIndex again when its
        // movement ends) is not moving.
        FrameAnimation {
            id: swipeSettle
            onTriggered: {
                const o = win.swipeOffset();
                const moving = !!(swipe.contentItem && swipe.contentItem.moving);
                if (!moving && o === swipe.currentIndex * swipe.width) {
                    stop();
                    win.lg("swipe", "rest", Math.round(o));
                }
            }
        }
        Button {
            id: swipeTo2
            x: 950; y: 470; width: 95; height: 30
            text: "Page 3"
            onClicked: win.swipeGo(2)
        }
        Button {
            id: swipeTo0
            x: 1055; y: 470; width: 95; height: 30
            text: "Page 1"
            onClicked: win.swipeGo(0)
        }

        // A touchpad's finger frames: both deltas of both axes, as the event carries them. Qt
        // opens a scroll with an event that carries none (phase ScrollBegin), which the shell
        // does not send: the frames that scroll are compared.
        Rectangle {
            x: 760; y: 640; width: 100; height: 60
            color: "#535"
            MouseArea {
                anchors.fill: parent
                acceptedButtons: Qt.NoButton
                onWheel: wheel => {
                    if (wheel.angleDelta.x !== 0 || wheel.angleDelta.y !== 0)
                        win.lg("pad", "wheel", "angle " + wheel.angleDelta.x + " " + wheel.angleDelta.y
                            + " pixel " + wheel.pixelDelta.x + " " + wheel.pixelDelta.y);
                }
            }
        }

        Button {
            id: dblBtn
            x: 950; y: 520; width: 200; height: 30
            text: "Double"
            onPressed: win.lg("dblBtn", "pressed")
            onReleased: win.lg("dblBtn", "released")
            onClicked: win.lg("dblBtn", "clicked")
            onDoubleClicked: win.lg("dblBtn", "doubleClicked")
        }

        // A long press runs the button's altAction and lets it up; the release that follows
        // is no click. the original's handler went on to write to the `clicked` signal, which Qt
        // refuses with a TypeError in the log (docs/original-differences.md).
        GroupButton {
            id: grp
            x: 950; y: 580; width: 200; height: 40
            altAction: () => win.lg("grp", "altAction")
            releaseAction: () => win.lg("grp", "release")
            onClicked: win.lg("grp", "clicked")
            onDownChanged: win.lg("grp", "down", down)
        }
    }
}

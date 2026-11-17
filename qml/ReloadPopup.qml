import QtQuick
import QtQuick.Layouts
import Qt5Compat.GraphicalEffects
import Rustshell
import Rustshell.Wayland
import ri

Scope {
	id: root
	property bool failed;
	property string errorString;

	// From the reload's signal until the popup is gone.
	readonly property bool shown: popupLoader.loading || popupLoader.active
	onShownChanged: GlobalStates.reloadPopupShown = shown

	// Connect to the Quickshell global to listen for the reload signals.
	Connections {
		target: Rustshell

		function onReloadCompleted() {
			root.failed = false;
			popupLoader.loading = true;
		}

		function onReloadFailed(error: string) {
			// Close any existing popup before making a new one.
			popupLoader.active = false;

			root.failed = true;
			root.errorString = error;
			popupLoader.loading = true;
		}
	}

	// Keep the popup in a loader because it isn't needed most of the time
	LazyLoader {
		id: popupLoader

		PanelWindow {
			id: popup

			exclusiveZone: 0
			anchors.top: true
			margins.top: 0

			implicitWidth: rect.width + shadow.radius * 2
			implicitHeight: rect.height + shadow.radius * 2

			WlrLayershell.namespace: "rustshell:reloadPopup"

			// The popup fades in as it comes and out before it goes: 0 unseen, 1 shown.
			// The fade and the countdown start once twelve frames in a row have each come on
			// time. A shell that has just started is still making things, and a frame it is late
			// with would be a step in the fade; the window's own first frame takes longer than a
			// frame too (as EditOverlay.qml's). After a second and a half they start anyway.
			property real shown: 0
			// What the popup is painted with while unseen, too faint to change a pixel: its first
			// painting (the text's glyphs, the shadow's layer) takes most of a frame on a slow
			// GPU, and is done in the frames counted below, not in the fade's first ones.
			readonly property real unseen: 0.001
			property int framesShown: 0
			property bool started: false
			FrameAnimation {
				running: !popup.started
				onTriggered: {
					// Late is well over what the frames have taken lately. The first ten are, the
					// smoothed time starting from nothing.
					const late = frameTime > smoothFrameTime * 1.6
					popup.framesShown = late ? 0 : popup.framesShown + 1
					if (popup.framesShown >= 12 || elapsedTime >= 1.5) {
						popup.started = true
						anim.start()
						fadeIn.start()
					}
				}
			}
			NumberAnimation {
				id: fadeIn
				target: popup
				property: "shown"
				from: 0
				to: 1
				duration: 500
				easing.type: Easing.InOutQuad
			}
			NumberAnimation {
				id: fadeOut
				target: popup
				property: "shown"
				to: 0
				duration: 700
				easing.type: Easing.InOutQuad
				onFinished: popupLoader.active = false
			}

			// color blending is a bit odd as detailed in the type reference.
			color: "transparent"

			Rectangle {
				id: rect
				anchors.centerIn: parent
				opacity: Math.max(popup.shown, popup.unseen)
				color: failed ?  "#ffe99195" : "#ffD1E8D5"

				implicitHeight: layout.implicitHeight + 30
				implicitWidth: layout.implicitWidth + 30
				radius: 12

				// Fills the whole area of the rectangle, making any clicks go to it,
				// which dismiss the popup.
				MouseArea {
					id: mouseArea
					anchors.fill: parent
					onPressed: {
						popupLoader.active = false
					}

					// makes the mouse area track mouse hovering, so the hide animation
					// can be paused when hovering.
					hoverEnabled: true
				}

				ColumnLayout {
					id: layout
					spacing: 10
					anchors {
						top: parent.top
						topMargin: 10
						horizontalCenter: parent.horizontalCenter
					}

					Text {
						renderType: Text.NativeRendering
						font.family: "Google Sans Flex"
						font.pointSize: 14
						text: root.failed ? "Shell: Reload failed" : "Shell reloaded"
						color: failed ? "#ff93000A" : "#ff0C1F13"
					}

					Text {
						renderType: Text.NativeRendering
						font.family: "JetBrains Mono NF"
						font.pointSize: 11
						text: root.errorString
						color: failed ? "#ff93000A" : "#ff0C1F13"
						// When visible is false, it also takes up no space.
						visible: root.errorString != ""
					}
				}

				// A progress bar on the bottom of the screen, showing how long until the
				// popup is removed.
				Rectangle {
					z: 2
					id: bar
					color: failed ? "#ff93000A" : "#ff0C1F13"
					anchors.bottom: parent.bottom
					anchors.left: parent.left
					anchors.margins: 10
					height: 5
					radius: 9999

					PropertyAnimation {
						id: anim
						target: bar
						property: "width"
						from: rect.width - bar.anchors.margins * 2
						to: 0
						duration: failed ? 10000 : 2500
						onFinished: fadeOut.start()

						// Pause the animation when the mouse is hovering over the popup,
						// so it stays onscreen while reading. This updates reactively
						// when the mouse moves on and off the popup.
						paused: mouseArea.containsMouse
					}
				}
				// Its bg
				Rectangle {
					z: 1
					id: bar_bg
					color: failed ? "#30af1b25" : "#4027643e"
					anchors.bottom: parent.bottom
					anchors.left: parent.left
					anchors.margins: 10
					height: 5
					radius: 9999
					width: rect.width - bar.anchors.margins * 2
				}

				// The width the countdown starts from is laid out by the time the frames above
				// have been drawn.
			}

			DropShadow {
				id: shadow
                anchors.fill: rect
                opacity: Math.max(popup.shown, popup.unseen)
                horizontalOffset: 0
                verticalOffset: 2
                radius: 6
                samples: radius * 2 + 1 // Ideally should be 2 * radius + 1, see qt docs
                color: "#44000000"
                source: rect
            }
		}
	}
}

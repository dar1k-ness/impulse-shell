import QtQuick
import QtQuick.Layouts
import Qt5Compat.GraphicalEffects
import Rustshell
import Rustshell.Wayland

Scope {
	id: root
	property bool failed;
	property string errorString;

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
			// The fade and the countdown start once the window has drawn a few frames (as
			// EditOverlay.qml's does): its first frame takes longer than a frame, and a fade
			// started with the component would be most of the way before it is seen.
			property real shown: 0
			property int framesShown: 0
			FrameAnimation {
				running: popup.framesShown < 12
				onTriggered: {
					popup.framesShown++
					if (popup.framesShown === 12) {
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
				opacity: popup.shown
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
                opacity: popup.shown
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

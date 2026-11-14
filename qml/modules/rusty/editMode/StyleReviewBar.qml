import QtQuick
import QtQuick.Layouts
import ri.services
import ri.modules.common
import ri.modules.common.widgets

/**
 * The bar of a style review (services/StyleReview.qml) at the top of the edit overlay: what
 * the picker is doing or said last, like, dislike (which picks another style) and confirm,
 * which leaves the edit mode. Loaded only while a review is open.
 */
Loader {
    id: root

    // The overlay's monitor.
    property string screenName: ""

    anchors.horizontalCenter: parent.horizontalCenter
    anchors.top: parent.top
    anchors.topMargin: Appearance.sizes.elevationMargin * 4 + (LayoutState.panelEdge(root.screenName) === "top" ? Appearance.sizes.barHeight : 0)
    active: StyleReview.open

    sourceComponent: Toolbar {
        spacing: 8

        Item {
            Layout.leftMargin: 8
            implicitWidth: 28
            implicitHeight: 28
            MaterialSymbol {
                anchors.centerIn: parent
                visible: !StyleReview.busy
                text: StyleReview.failed ? "error" : "wallpaper_slideshow"
                iconSize: 24
                color: StyleReview.failed ? Appearance.colors.colError : Appearance.colors.colPrimary
            }
            MaterialLoadingIndicator {
                anchors.centerIn: parent
                visible: StyleReview.busy
                loading: StyleReview.busy
                implicitSize: 28
            }
        }

        ColumnLayout {
            Layout.rightMargin: 8
            Layout.alignment: Qt.AlignVCenter
            spacing: 0
            StyledText {
                text: StyleReview.busy ? Translation.tr("Picking another style") : Translation.tr("New style")
                color: Appearance.colors.colOnSurface
                font.pixelSize: Appearance.font.pixelSize.small
                font.weight: Font.DemiBold
            }
            StyledText {
                Layout.maximumWidth: 300
                elide: Text.ElideRight
                text: StyleReview.status.length > 0 ? StyleReview.status : StyleReview.busy ? Translation.tr("Looking...") : Translation.tr("Arrange the widgets, then confirm")
                color: StyleReview.failed ? Appearance.colors.colError : Appearance.colors.colSubtext
                font.pixelSize: Appearance.font.pixelSize.smaller
            }
        }

        IconAndTextToolbarButton {
            focusPolicy: Qt.NoFocus
            enabled: !StyleReview.busy
            toggled: StyleReview.liked
            iconText: "thumb_up"
            text: StyleReview.liked ? Translation.tr("Liked") : Translation.tr("Like")
            onClicked: StyleReview.like()
            StyledToolTip {
                text: Translation.tr("More like this")
            }
        }
        IconAndTextToolbarButton {
            focusPolicy: Qt.NoFocus
            enabled: !StyleReview.busy
            iconText: "thumb_down"
            text: Translation.tr("Dislike")
            onClicked: StyleReview.dislike()
            StyledToolTip {
                text: Translation.tr("Less like this, and pick another")
            }
        }
        IconAndTextToolbarButton {
            focusPolicy: Qt.NoFocus
            enabled: !StyleReview.busy
            toggled: true
            colBackgroundToggled: Appearance.colors.colPrimary
            colBackgroundToggledHover: Appearance.colors.colPrimaryHover
            colRippleToggled: Appearance.colors.colPrimaryActive
            // Disabled, a button has no background: the text on the toolbar itself.
            colText: enabled ? Appearance.colors.colOnPrimary : Appearance.colors.colOnSurfaceVariant
            iconText: "check"
            text: Translation.tr("Confirm")
            onClicked: StyleReview.confirm()
        }
    }
}

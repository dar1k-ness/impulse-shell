//@ pragma UseQApplication
//@ pragma Env QT_QUICK_CONTROLS_STYLE=Basic
import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import Quickshell
import qs.modules.common
import qs.modules.common.widgets
import qs.modules.ii.onScreenDisplay
ApplicationWindow {
    width: 700; height: 420; visible: true
    color: Appearance.m3colors.m3background
    ColumnLayout {
        anchors.fill: parent; anchors.margins: 10; spacing: 8
        RowLayout {
            spacing: 4
            OsdValueIndicator { value: 0.5; icon: "volume_up"; name: "Volume" }
            OsdValueIndicator { value: 0.7; icon: "light_mode"; name: "Brightness"; rotateIcon: true; scaleIcon: true }
            OsdValueIndicator { value: 0.6; icon: "wb_twilight"; name: "Gamma"; from: 0.25 }
        }
        Rectangle {
            Layout.preferredWidth: 220; Layout.preferredHeight: 60; color: "#40000000"
            SqueezedAnnotationStyledText { anchors.fill: parent; text: "Größe"; color: "white" }
        }
        MaterialTextField { Layout.preferredWidth: 300; placeholderText: "Translator input" }
        StyledTextArea { Layout.preferredWidth: 300; padding: 10; placeholderText: "Спросите ассистента..."; wrapMode: TextArea.Wrap }
        StyledTextArea { Layout.preferredWidth: 300; padding: 10; text: "Текст в поле\nвторая строка"; wrapMode: TextArea.Wrap }
    }
}

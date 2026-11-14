import QtQuick
import QtQuick.Controls

StackView {
    property alias inner: inside

    StackView { id: inside }
}

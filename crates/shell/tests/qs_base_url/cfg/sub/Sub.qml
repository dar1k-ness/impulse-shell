import QtQuick
import QtQuick.Controls
Text {
    property alias inner: i
    property alias area: a
    Text { id: i }
    TextArea { id: a }
}

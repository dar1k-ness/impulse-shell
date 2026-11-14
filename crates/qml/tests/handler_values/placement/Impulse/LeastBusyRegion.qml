import QtQuick
// Stands for the native search.
QtObject {
    property string imagePath
    property int regionWidth
    property int regionHeight
    property int horizontalPadding
    property int verticalPadding
    property int screenWidth
    property int screenHeight
    property bool busiest
    property bool running
    signal found(var region)
}

// cell 260x90 cols 2: TextField TextFieldText
import QtQuick
import QtQuick.Controls.FluentWinUI3
import QtQuick.Controls

Grid {
    columns: 2
    component Cell: Item { width: 260; height: 90 }
    // qml/modules/waffle/looks/WTextField.qml:6
    Cell { TextField { x: 10; y: 10; width: 240; clip: true; renderType: Text.NativeRendering; verticalAlignment: Text.AlignVCenter; font.pixelSize: 11; color: "white"; placeholderText: "Password" } }
    Cell { TextField { x: 10; y: 10; width: 240; clip: true; renderType: Text.NativeRendering; verticalAlignment: Text.AlignVCenter; font.pixelSize: 11; color: "white"; text: "secret"; echoMode: TextInput.Password } }
}

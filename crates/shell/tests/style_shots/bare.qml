// cell 220x90 cols 4: Button ComboBox ItemDelegate MenuItem RadioButton ScrollBar Slider SpinBox Switch TabBar TextField TextFieldSel TextArea DayOfWeekRow CheckBox ProgressBar
import QtQuick
import QtQuick.Controls

Grid {
    columns: 4
    component Cell: Item { width: 220; height: 90 }
    Cell { Button { x: 10; y: 10; text: "Button" } }
    Cell { ComboBox { x: 10; y: 10; model: ["one", "two"] } }
    Cell { ItemDelegate { x: 10; y: 10; text: "Delegate"; highlighted: true } }
    Cell { MenuItem { x: 10; y: 10; text: "Item"; checkable: true; checked: true; highlighted: true } }
    Cell { RadioButton { x: 10; y: 10; text: "Radio"; checked: true } }
    Cell { ScrollBar { x: 10; y: 5; height: 80; size: 0.3; position: 0.2; policy: ScrollBar.AlwaysOn } }
    Cell { Slider { x: 10; y: 10; value: 0.4 } }
    Cell { SpinBox { x: 10; y: 10; value: 5 } }
    Cell { Switch { x: 10; y: 10; text: "Switch"; checked: true } }
    Cell { TabBar { x: 10; y: 10; width: 200; TabButton { text: "One" } TabButton { text: "Two" } } }
    Cell { TextField { x: 10; y: 10; placeholderText: "Placeholder" } }
    Cell { TextField { x: 10; y: 10; text: "Selected text"; cursorVisible: true; Component.onCompleted: select(0, 8) } }
    Cell { TextArea { x: 10; y: 10; width: 200; placeholderText: "Area placeholder" } }
    Cell { DayOfWeekRow { x: 10; y: 10; locale: Qt.locale("en_US") } }
    Cell { CheckBox { x: 10; y: 10; text: "Check"; checked: true } }
    Cell { ProgressBar { x: 10; y: 40; value: 0.4 } }
}

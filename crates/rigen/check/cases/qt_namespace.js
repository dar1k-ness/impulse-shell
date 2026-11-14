// Qt.platform and enum values of the Qt namespace (QtCore's Qt enums, read through QML).
// v4: ["object","linux","string",0,1,132,16777216,-33554432,33554432,33554431,1,0,120,-1,null,1,65,"object","undefined",33554432]
var r = [typeof Qt.platform, Qt.platform.os, typeof Qt.platform.pluginName];
r.push(Qt.Asynchronous, Qt.Synchronous, Qt.AlignCenter, Qt.Key_Escape, Qt.KeyboardModifierMask);
r.push(Qt.ShiftModifier, Qt.Key_unknown, Qt.ElideRight, Qt.PreciseTimer, Qt.WA_TranslucentBackground);
r.push(Object.keys(Qt).indexOf("AlignLeft"), Qt.NoSuchEnum);
r.push(Qt.Alignment.AlignLeft, Qt.Key.Key_A, typeof Qt.GlobalColor, typeof Qt.AlignmentFlag, Qt.KeyboardModifiers.ShiftModifier);
var result = JSON.stringify(r);

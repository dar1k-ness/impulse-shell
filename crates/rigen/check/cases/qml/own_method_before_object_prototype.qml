// An object's own method is found before %Object.prototype%'s of the same name: Quickshell's
// `PamResult` has a `toString(result)` of its own (runtime_audit_r3.rs,
// `r3_06_own_method_before_object_prototype`, holds the same for the interpreter).
// qt: Failed function
import QtQuick
import Rustshell.Services.Pam

Item {
    property string result: PamResult.toString(PamResult.Failed) + " " + typeof PamResult.hasOwnProperty
}

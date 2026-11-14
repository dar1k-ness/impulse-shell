import QtQuick
Item {
    TextInput { id: b; validator: DoubleValidator { id: dv; bottom: 0; top: 1; decimals: 2 } text: "0.555" }
    TextInput { id: g; validator: DoubleValidator { id: dg; bottom: -5; top: 500 } }
    function t(f, s) { f.text = s; console.log(JSON.stringify(s), f.acceptableInput, JSON.stringify(f.text)) }
    Component.onCompleted: {
        console.log("notation", dv.notation, DoubleValidator.StandardNotation, DoubleValidator.ScientificNotation, dv.decimals, dg.decimals, dg.notation, dv.bottom, dv.top, dg.bottom, dg.top)
        for (const s of ["1e0", "1E0", "1e-1", "1e+0", "5e-1", "0.5e0", "0.55e0", "0.555e0", "1e", "1e1", "e1", "1e0.5", "10e-1", "1.e0", ".5", "5.", "0.50", "0.5e1", "1e00", "1e-2", "100e-2", "1e-3", "0.01e2","0.001e3"]) t(b, s)
        dv.notation = DoubleValidator.StandardNotation
        for (const s of ["1e0", "0.5", "0.55", "0.555", "1.5", "10"]) t(b, s)
        for (const s of ["1e2", "5e2", "6e2", "1e3", "-1e1", "-5e0", "-6e0", "5.5e2", "4.99999e2", "1,000e-1", "1e2,0", "+1e2", "1e+2", "5e+2", "1e1000", "1e-1000", "0x10", " 1", "1 "]) t(g, s)
    }
}

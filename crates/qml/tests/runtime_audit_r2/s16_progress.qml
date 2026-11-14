import QtQuick
import QtQuick.Controls
Item {
    ProgressBar { id: p; value: 5; to: 2 }
    ProgressBar { id: q; from: 10; to: 0; value: 4 }
    ProgressBar { id: z; from: 3; to: 3; value: 3 }
    ProgressBar { id: m; LayoutMirroring.enabled: true; value: 0.25 }
    Component.onCompleted: {
        console.log("p", p.value, p.position, p.visualPosition)
        console.log("q", q.value, q.position, q.visualPosition)
        console.log("z", z.value, z.position)
        console.log("m", m.position, m.visualPosition)
        p.value = -3; console.log("p-", p.value, p.position)
        p.to = 10; p.value = 7; p.to = 5; console.log("p to shrink", p.value, p.position)
        p.indeterminate = true; console.log("ind", p.position)
    }
}

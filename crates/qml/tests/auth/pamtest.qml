import QtQuick
import Rustshell.Services.Pam
Item { id: r
    property string input
    property int inputSeq
    property bool quit
    property string pw
    property int n: 0
    PamContext { id: pam
        config: "ri-native-test"
        onPamMessage: {
            console.log("pamMessage", JSON.stringify(this.message), "isError", this.messageIsError, "required", this.responseRequired, "visible", this.responseVisible);
            if (this.responseRequired) { const v = r.pw.split(",")[r.n++] ?? ""; this.respond(v) }
        }
        onCompleted: result => console.log("completed", result, PamResult.toString(result), "active", pam.active)
        onError: error => console.log("error", error, PamError.toString(error))
        onActiveChanged: console.log("active", active)
    }
    onInputSeqChanged: {
        const parts = input.split(" ");
        if (parts[0] === "try") { r.pw = parts[1] ?? ""; r.n = 0; console.log("start() ->", pam.start()) }
        else if (parts[0] === "config") { pam.config = parts[1]; console.log("config =", pam.config) }
        else if (parts[0] === "user") { pam.user = parts[1] ?? ""; console.log("user =", pam.user) }
        else if (parts[0] === "abort") { pam.abort(); console.log("abort(), active", pam.active, "message", JSON.stringify(pam.message)) }
        else if (parts[0] === "quit") quit = true
    }
}

import QtQuick
import Rustshell.Services.Polkit
Item { id: r
    property string input
    property int inputSeq
    property bool quit
    PolkitAgent { id: agent
        onIsRegisteredChanged: console.log("isRegistered", isRegistered, "path", path)
        onIsActiveChanged: console.log("isActive", isActive)
        onAuthenticationRequestStarted: {
            const f = agent.flow;
            console.log("authenticationRequestStarted action", f.actionId, "message", JSON.stringify(f.message), "icon", JSON.stringify(f.iconName), "cookie?", f.cookie.length > 0);
            console.log("  identities", f.identities.map(i => i.string + "/" + i.displayName + " id=" + i.id + " group=" + i.isGroup).join(", "), "selected", f.selectedIdentity.string);
        }
    }
    Connections { target: agent.flow
        function onIsResponseRequiredChanged() { if (agent.flow.isResponseRequired) console.log("prompt", JSON.stringify(agent.flow.inputPrompt), "responseVisible", agent.flow.responseVisible) }
        function onSupplementaryMessageChanged() { console.log("supplementary", JSON.stringify(agent.flow.supplementaryMessage), "isError", agent.flow.supplementaryIsError) }
        function onAuthenticationFailed() { console.log("authenticationFailed failed=", agent.flow.failed) }
        function onIsCompletedChanged() { console.log("isCompleted", agent.flow.isCompleted, "isSuccessful", agent.flow.isSuccessful, "isCancelled", agent.flow.isCancelled) }
    }
    property var last: null
    Connections { target: agent
        function onFlowChanged() { if (agent.flow) r.last = agent.flow }
    }
    Connections { target: r.last
        function onAuthenticationSucceeded() { console.log("authenticationSucceeded") }
        function onAuthenticationRequestCancelled() { console.log("authenticationRequestCancelled (by polkitd)") }
    }
    onInputSeqChanged: {
        if (input === "cancel") { console.log("cancelAuthenticationRequest()"); agent.flow.cancelAuthenticationRequest(); console.log("  flow now", agent.flow, "isActive", agent.isActive) }
        else if (input === "quit") quit = true
        else { console.log("submit(<" + input.length + " chars>)"); agent.flow.submit(input) }
    }
}

pragma ComponentBehavior: Bound

import QtQuick
import Rustshell
import Rustshell.Io

/**
 * One conversation with an agent run as a subprocess (`command`), spoken to over the Agent Client
 * Protocol: newline-delimited JSON-RPC 2.0 on stdio (https://agentclientprotocol.com/).
 *
 * While an agent asked to end is still running, the chat has a line saying so with a button
 * that kills it, and the prompts sent meanwhile wait for the agent started after it
 * (docs/original-differences.md).
 */
Item {
    id: root

    property list<string> command: []
    readonly property string program: command.length > 0 ? command[0].split("/").pop() : ""
    property string startedCommand: ""
    // The program of the agent that was started last: `program` is the configured one.
    property string startedProgram: ""
    // A config reload reassigns the same list: restart only for a different agent.
    onCommandChanged: if (proc.running && JSON.stringify(root.command) !== root.startedCommand) root.restart()
    property string cwd: Rustshell.env("HOME")
    property string preferredMode: ""
    property string preferredModel: ""

    signal responseFinished()
    signal modelChosen(string model)
    signal modeChosen(string mode)

    readonly property bool running: proc.running
    // The running agent was asked to end and has not exited.
    readonly property bool waitingForExit: root.stopping && proc.running
    property bool initialized: false
    property string sessionId: ""
    property bool busy: false
    property bool authRequired: false
    property var authMethods: []
    property string agentName: ""
    property var models: []
    property string currentModel: ""
    property var modes: []
    property string currentMode: ""
    property var messages: []

    readonly property string currentModelName: models.find(m => m.value === currentModel)?.name ?? currentModel
    readonly property string currentModeName: modes.find(m => m.value === currentMode)?.name ?? currentMode

    property int nextRequestId: 1
    property var callbacks: ({})
    property var toolMessages: ({})
    property var currentAssistant: null
    property var currentThought: null
    property var currentPlan: null
    property var pendingPrompts: []
    // `proc.running = false` only asks the agent to end (SIGTERM): it runs until `exited`.
    // stopping: this session asked (`end`), so the exit is not reported. startAfterExit: the
    // agent in `command` is started from that exit, and until then the chat has exitNotice,
    // the line that says what is waited for.
    property bool stopping: false
    property bool startAfterExit: false
    property var exitNotice: null
    // The agent's `sessionCapabilities.close`: it takes `session/close`.
    property bool closesSessions: false
    // The last line the agent wrote that is no JSON while no session was open: part of the
    // report if it exits without opening one.
    property string strayOutput: ""

    Component {
        id: messageComponent
        AcpMessageData {}
    }

    function addMessage(role, content, props) {
        const message = messageComponent.createObject(root, Object.assign({ role: role, content: content ?? "", model: root.currentModelName, agent: root.agentName }, props ?? {}));
        root.messages = [...root.messages, message];
        return message;
    }

    function finishStreaming() {
        for (const m of [root.currentAssistant, root.currentThought, root.currentPlan]) {
            if (m) m.done = true;
        }
        root.currentAssistant = null;
        root.currentThought = null;
    }

    // ---- JSON-RPC plumbing ----
    function send(obj) {
        proc.write(JSON.stringify(obj) + "\n");
    }
    function request(method, params, callback) {
        const id = root.nextRequestId++;
        root.callbacks[id] = callback;
        root.send({ jsonrpc: "2.0", id: id, method: method, params: params });
    }
    function notify(method, params) {
        root.send({ jsonrpc: "2.0", method: method, params: params });
    }
    function respond(id, result) {
        root.send({ jsonrpc: "2.0", id: id, result: result });
    }
    function respondError(id, code, message) {
        root.send({ jsonrpc: "2.0", id: id, error: { code: code, message: message } });
    }

    function handleLine(line) {
        let msg;
        try {
            msg = JSON.parse(line);
        } catch (e) {
            // Agents that also log to stdout work, so the line is only kept.
            if (!root.sessionId) root.strayOutput = line.slice(0, 200);
            return;
        }
        if (msg.method !== undefined && msg.id !== undefined) {
            root.handleRequest(msg.id, msg.method, msg.params ?? {});
        } else if (msg.method !== undefined) {
            root.handleNotification(msg.method, msg.params ?? {});
        } else if (msg.id !== undefined) {
            const callback = root.callbacks[msg.id];
            delete root.callbacks[msg.id];
            if (callback) callback(msg.result, msg.error);
        }
    }

    // ---- Lifecycle ----
    // https://agentclientprotocol.com/protocol/initialization
    function start() {
        if (root.command.length === 0) return;
        if (proc.running) {
            if (root.stopping) root.awaitExit();
            return;
        }
        root.initialized = false;
        root.closesSessions = false;
        root.strayOutput = "";
        root.startedCommand = JSON.stringify(root.command);
        root.startedProgram = root.program;
        proc.stdinEnabled = true;
        proc.running = true;
        // A program that cannot be run is not running after this and sends no `exited`.
        if (!proc.running) {
            if (root.pendingPrompts.length > 0) root.addMessage("interface", `${root.program} failed to start`, { done: true });
            root.dropQueued();
            return;
        }
        root.request("initialize", {
            protocolVersion: 1,
            clientCapabilities: { fs: { readTextFile: false, writeTextFile: false }, terminal: false },
            clientInfo: { name: "rusty-impulse", version: "1.0" }
        }, (result, error) => {
            if (root.stopping) return;
            if (error) {
                root.showError(error, "initialize");
                root.sessionRefused(error);
                return;
            }
            root.agentName = result?.agentInfo?.title ?? result?.agentInfo?.name ?? "";
            root.authMethods = result?.authMethods ?? [];
            root.closesSessions = !!result?.agentCapabilities?.sessionCapabilities?.close;
            root.initialized = true;
            root.newSession();
        });
    }

    // ACP's auth_required error (-32000) on session/new or session/prompt.
    function isAuthError(error) {
        return !error.agentExited && (error.code === -32000 || /authenticat|log ?in/i.test(error.message));
    }

    // An error answer, shown in the chat. The requests an agent leaves unanswered by exiting
    // are answered with `agentExited`: onExited reports that exit, once.
    function showError(error, prefix) {
        if (error.agentExited) return;
        root.addMessage("interface", prefix ? `${prefix}: ${error.message}` : error.message, { done: true });
    }

    // The prompts queued for a session that will not open have their end.
    function dropQueued() {
        if (root.pendingPrompts.length === 0) return;
        root.pendingPrompts = [];
        root.unmarkQueued();
        root.responseFinished();
    }

    function unmarkQueued() {
        for (const m of root.messages) {
            if (m.queued) m.queued = false;
        }
    }

    // The agent answered `initialize` or `session/new` with an error: it holds no
    // conversation, so it is ended and the next prompt starts it afresh.
    function sessionRefused(error) {
        if (error.agentExited) return;
        root.dropQueued();
        root.end();
    }

    // Asks the running agent to end, in ACP's order. Its session is closed first, if it
    // closes sessions (https://agentclientprotocol.com/protocol/session-setup#closing-active-sessions);
    // when that is answered, its stdin is closed and it gets SIGTERM
    // (https://agentclientprotocol.com/protocol/transports#stdio). What it answers from here
    // on opens no session: the one it had is over.
    function end() {
        if (!proc.running || root.stopping) return;
        const session = root.sessionId;
        root.stopping = true;
        root.initialized = false;
        root.sessionId = "";
        root.authRequired = false;
        if (!session || !root.closesSessions) {
            root.terminate();
            return;
        }
        // An agent that exits instead of answering needs neither.
        root.request("session/close", { sessionId: session }, (result, error) => {
            if (!error?.agentExited) root.terminate();
        });
    }

    function terminate() {
        proc.stdinEnabled = false;
        proc.running = false;
    }

    // A start waits for the exit of the agent asked to end: the chat says so until then.
    function awaitExit() {
        root.startAfterExit = true;
        if (!root.exitNotice) root.exitNotice = root.addMessage("ending", root.startedProgram);
    }

    // The button of that line: an agent that does not end when asked is killed. Its exit
    // goes the way of any other.
    function kill() {
        if (proc.running) proc.signal(9);
    }

    // https://agentclientprotocol.com/protocol/initialization#authentication
    function authenticate(methodId) {
        root.request("authenticate", { methodId: methodId }, (result, error) => {
            if (error) {
                root.showError(error, "authenticate");
                return;
            }
            root.authRequired = false;
            root.newSession();
        });
    }

    function newSession() {
        if (root.stopping) return;
        root.sessionId = "";
        root.request("session/new", { cwd: root.cwd, mcpServers: [] }, (result, error) => {
            if (root.stopping) return;
            if (error) {
                if (root.isAuthError(error)) {
                    root.authRequired = true;
                } else {
                    root.showError(error);
                    root.sessionRefused(error);
                }
                return;
            }
            root.authRequired = false;
            root.sessionId = result.sessionId;
            root.strayOutput = "";
            root.modelOptionId = "";
            root.modeOptionId = "";
            if (result.modes) {
                root.currentMode = result.modes.currentModeId;
                if (root.modes.length === 0)
                    root.modes = result.modes.availableModes.map(m => ({ value: m.id, name: m.name, description: m.description ?? "" }));
            }
            root.applyConfigOptions(result.configOptions ?? []);
            if (root.preferredMode && root.preferredMode !== root.currentMode) root.setMode(root.preferredMode);
            if (root.preferredModel && root.preferredModel !== root.currentModel) root.setModel(root.preferredModel);
            const queued = root.pendingPrompts;
            root.pendingPrompts = [];
            root.unmarkQueued();
            for (const text of queued) root.doPrompt(text);
        });
    }

    // Session config options (https://agentclientprotocol.com/protocol/session-config-options)
    // win over the older session modes for the mode selector when an agent sends both.
    property string modelOptionId: ""
    property string modeOptionId: ""

    function applyConfigOptions(configOptions) {
        const flatten = options => options.reduce((acc, o) => acc.concat(o.options ? flatten(o.options) : [o]), []);
        for (const option of configOptions) {
            const values = flatten(option.options ?? []).map(o => ({ value: o.value, name: o.name, description: o.description ?? "" }));
            const category = option.category ?? option.id;
            if (category === "model") {
                root.modelOptionId = option.id;
                if (values.length > 0) root.models = values;
                root.currentModel = option.currentValue;
            } else if (category === "mode") {
                root.modeOptionId = option.id;
                if (values.length > 0) root.modes = values;
                root.currentMode = option.currentValue;
            }
        }
    }

    function setConfigOption(configId, value) {
        if (!root.sessionId) return;
        root.request("session/set_config_option", { sessionId: root.sessionId, configId: configId, value: value }, (result, error) => {
            if (error) root.showError(error, configId);
            else if (result?.configOptions) root.applyConfigOptions(result.configOptions);
        });
    }

    function setModel(value) {
        const model = root.models.find(m => m.value === value || m.name.toLowerCase() === value.toLowerCase());
        if (!model || !root.modelOptionId) return;
        root.preferredModel = model.value;
        root.modelChosen(model.value);
        root.setConfigOption(root.modelOptionId, model.value);
    }

    function setMode(value) {
        if (!root.modes.some(m => m.value === value) || !root.sessionId) return;
        root.preferredMode = value;
        root.modeChosen(value);
        if (root.modeOptionId) {
            root.setConfigOption(root.modeOptionId, value);
            return;
        }
        // https://agentclientprotocol.com/protocol/session-modes
        root.request("session/set_mode", { sessionId: root.sessionId, modeId: value }, (result, error) => {
            if (error) root.showError(error, "session/set_mode");
            else root.currentMode = value;
        });
    }

    // The agent in `command` replaces the running one once that has exited. The session ends
    // here: the prompts queued and those sent until then are the new agent's to answer.
    function restart() {
        root.cancel();
        if (!proc.running) {
            root.start();
            return;
        }
        root.awaitExit();
        root.end();
    }

    function sendPrompt(text) {
        const message = root.addMessage("user", text, { done: true });
        if (!root.sessionId) {
            message.queued = true;
            root.pendingPrompts = [...root.pendingPrompts, text];
            root.start();
            return;
        }
        root.doPrompt(text);
    }

    function doPrompt(text) {
        root.busy = true;
        root.finishStreaming();
        root.currentPlan = null;
        root.request("session/prompt", { sessionId: root.sessionId, prompt: [{ type: "text", text: text }] }, (result, error) => {
            root.settleTurn(result?.stopReason === "cancelled" ? "cancelled" : "");
            if (error) {
                if (root.isAuthError(error)) root.authRequired = true;
                root.showError(error);
            } else if (result?.stopReason && result.stopReason !== "end_turn") {
                root.addMessage("interface", `Stopped: ${result.stopReason}`, { done: true });
            }
            root.responseFinished();
        });
    }

    // The end of a turn: nothing streams any more and no tool call is left open. One still
    // pending or running gets `unfinished` as its status, if given.
    function settleTurn(unfinished) {
        root.busy = false;
        root.finishStreaming();
        for (const id in root.toolMessages) {
            const tool = root.toolMessages[id];
            if (unfinished && (tool.status === "pending" || tool.status === "in_progress")) tool.status = unfinished;
            tool.done = true;
        }
        root.toolMessages = {};
    }

    function cancel() {
        for (const m of root.messages) {
            if (m.role === "permission" && !m.done) {
                root.respond(m.requestId, { outcome: { outcome: "cancelled" } });
                m.resolved = "Cancelled";
                m.done = true;
            }
        }
        if (root.sessionId && root.busy) root.notify("session/cancel", { sessionId: root.sessionId });
    }

    function clear() {
        root.cancel();
        // The line of an agent still waited for stays, with its button.
        for (const m of root.messages) {
            if (m !== root.exitNotice) m.destroy();
        }
        root.messages = root.exitNotice ? [root.exitNotice] : [];
        root.toolMessages = {};
        root.currentPlan = null;
        if (root.initialized) root.newSession();
    }

    function answerPermission(message, optionId) {
        if (message.done) return;
        root.respond(message.requestId, { outcome: { outcome: "selected", optionId: optionId } });
        message.resolved = message.options.find(o => o.optionId === optionId)?.name ?? optionId;
        message.done = true;
    }

    // ---- Incoming messages ----
    function contentToText(content) {
        if (!content) return "";
        const items = Array.isArray(content) ? content : [content];
        return items.map(item => {
            if (item.type === "text") return item.text;
            if (item.type === "content") return root.contentToText(item.content);
            if (item.type === "diff") return `${item.path}\n` + (item.newText ?? "").split("\n").slice(0, 40).map(l => `+ ${l}`).join("\n");
            if (item.type === "resource_link") return item.uri;
            return "";
        }).filter(t => t.length > 0).join("\n");
    }

    function rawInputToText(rawInput) {
        if (!rawInput) return "";
        if (typeof rawInput === "string") return rawInput;
        if (rawInput.command) return Array.isArray(rawInput.command) ? rawInput.command.join(" ") : rawInput.command;
        return JSON.stringify(rawInput, null, 2);
    }

    function handleNotification(method, params) {
        if (method !== "session/update" || params.sessionId !== root.sessionId) return;
        const update = params.update;
        switch (update.sessionUpdate) {
        case "agent_message_chunk": {
            if (root.currentThought) {
                root.currentThought.done = true;
                root.currentThought = null;
            }
            if (!root.currentAssistant) root.currentAssistant = root.addMessage("assistant", "");
            root.currentAssistant.content += root.contentToText(update.content);
            break;
        }
        case "agent_thought_chunk": {
            if (!root.currentThought) root.currentThought = root.addMessage("thought", "", { thinking: true });
            root.currentThought.content += root.contentToText(update.content);
            break;
        }
        case "tool_call": {
            root.finishStreaming();
            const tool = root.addMessage("tool", "", {
                toolCallId: update.toolCallId,
                title: update.title ?? "",
                kind: update.kind ?? "other",
                status: update.status ?? "pending",
                detail: root.rawInputToText(update.rawInput),
                content: root.contentToText(update.content)
            });
            root.toolMessages[update.toolCallId] = tool;
            break;
        }
        case "tool_call_update": {
            const tool = root.toolMessages[update.toolCallId];
            if (!tool) break;
            if (update.title) tool.title = update.title;
            if (update.kind) tool.kind = update.kind;
            if (update.status) tool.status = update.status;
            if (update.rawInput) tool.detail = root.rawInputToText(update.rawInput);
            if (update.content) tool.content = root.contentToText(update.content);
            if (update.status === "completed" || update.status === "failed") tool.done = true;
            break;
        }
        case "plan": {
            const icons = { completed: "[x]", in_progress: "[~]", pending: "[ ]" };
            const text = (update.entries ?? []).map(e => `- ${icons[e.status] ?? "[ ]"} ${e.content}`).join("\n");
            if (!root.currentPlan) root.currentPlan = root.addMessage("plan", text);
            else root.currentPlan.content = text;
            break;
        }
        case "config_option_update":
            root.applyConfigOptions(update.configOptions ?? []);
            break;
        case "current_mode_update":
            root.currentMode = update.currentModeId;
            break;
        }
    }

    function handleRequest(id, method, params) {
        if (method === "session/request_permission") {
            const toolCall = params.toolCall ?? {};
            const known = root.toolMessages[toolCall.toolCallId];
            root.finishStreaming();
            root.addMessage("permission", root.contentToText(toolCall.content), {
                requestId: id,
                title: toolCall.title ?? known?.title ?? "Permission required",
                kind: toolCall.kind ?? known?.kind ?? "other",
                detail: root.rawInputToText(toolCall.rawInput) || known?.detail || "",
                options: params.options ?? []
            });
            return;
        }
        root.respondError(id, -32601, `Method not supported: ${method}`);
    }

    Process {
        id: proc
        command: root.command
        workingDirectory: root.cwd
        stdinEnabled: true
        stdout: SplitParser {
            splitMarker: "\n"
            onRead: data => root.handleLine(data)
        }
        onExited: (exitCode, exitStatus) => {
            const wasBusy = root.busy;
            const asked = root.stopping;
            const again = root.startAfterExit;
            const queued = root.pendingPrompts.length > 0;
            const unanswered = root.callbacks;
            const notice = root.exitNotice;
            root.stopping = false;
            root.startAfterExit = false;
            if (notice) {
                root.exitNotice = null;
                root.messages = root.messages.filter(m => m !== notice);
                notice.destroy();
            }
            root.initialized = false;
            root.sessionId = "";
            root.callbacks = {};
            // What the agent left open is closed: its turn, the tool calls it was running and
            // the permissions it asked for.
            root.settleTurn(asked ? "cancelled" : "failed");
            for (const m of root.messages) {
                if (m.role === "permission" && !m.done) {
                    m.resolved = "Cancelled";
                    m.done = true;
                }
            }
            const exited = `${root.startedProgram} exited (code ${exitCode})`;
            if (!asked && (wasBusy || exitCode !== 0 || queued))
                root.addMessage("interface", root.strayOutput ? `${exited}: ${root.strayOutput}` : exited, { done: true });
            // Each request it left unanswered is answered, as an error answer of its would.
            for (const id in unanswered) unanswered[id](undefined, { code: -32603, message: exited, agentExited: true });
            if (again) root.start();
            else root.dropQueued();
        }
    }

    Component.onDestruction: if (proc.running) proc.running = false
}

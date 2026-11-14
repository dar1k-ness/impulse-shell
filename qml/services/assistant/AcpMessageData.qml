import QtQuick

/**
 * A single entry in an ACP conversation (services/assistant/AcpSession.qml).
 * role: user | assistant | thought | tool | permission | plan | interface | ending
 * (ending: the agent named in `content` was asked to end and is waited for)
 */
QtObject {
    property string role
    property string content
    property bool done: false
    property bool thinking: false
    property string model
    // The agent that wrote an answer, as it named itself: a later agent's name is not put on it
    property string agent
    // A prompt that waits for a session to open
    property bool queued: false
    // Tool calls
    property string toolCallId
    property string title
    property string kind
    property string status
    property string detail
    // Permission requests
    property var requestId: null
    property var options: []
    property string resolved
}

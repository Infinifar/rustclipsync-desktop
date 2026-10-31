import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ApplicationWindow {
    id: window
    title: qsTr("敏感收件区")
    width: 420
    height: 320
    minimumWidth: 360
    minimumHeight: 260
    visible: false
    property bool pinned: false
    property var inbox: backend.status.inbox || ({})
    flags: Qt.Window | (pinned ? Qt.WindowStaysOnTopHint : 0)
    font.pixelSize: 14 * backend.textScale
    palette: parentPalette
    property var parentPalette
    onVisibleChanged: if (!visible) backend.hideInbox()
    onActiveChanged: if (!active) backend.hideInbox()
    onClosing: function(close) { close.accepted = false; hide(); backend.hideInbox() }
    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 20
        spacing: 12
        RowLayout {
            Layout.fillWidth: true
            Label { text: qsTr("敏感收件区"); font.bold: true; Layout.fillWidth: true }
            Button { text: window.pinned ? qsTr("取消置顶") : qsTr("置顶"); checkable: true; checked: window.pinned; onClicked: window.pinned = checked }
        }
        ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            contentWidth: availableWidth
            TextArea {
                text: !window.inbox.id ? qsTr("尚未收到敏感文本") : (backend.inboxText || "••••••••")
                readOnly: true
                selectByMouse: false
                wrapMode: TextEdit.WrapAnywhere
                Accessible.name: backend.inboxText ? qsTr("已显示的敏感文本") : qsTr("隐藏的敏感文本")
            }
        }
        RowLayout {
            Layout.fillWidth: true
            Label { text: window.inbox.id ? backend.receivedTime(window.inbox.receivedAt) : ""; Layout.fillWidth: true; opacity: 0.7 }
            Button { flat: true; text: backend.inboxText ? qsTr("隐藏") : qsTr("显示"); enabled: !!window.inbox.id && !backend.busy; onClicked: backend.revealInbox() }
        }
        RowLayout {
            Button { text: qsTr("复制到剪贴板"); enabled: !!window.inbox.id && !backend.busy; onClicked: backend.invoke("copyInbox", {id: window.inbox.id}) }
            Button { flat: true; text: qsTr("删除"); enabled: !!window.inbox.id && !backend.busy; onClicked: { backend.hideInbox(); backend.invoke("clearInbox", {id: window.inbox.id}) } }
        }
    }
}

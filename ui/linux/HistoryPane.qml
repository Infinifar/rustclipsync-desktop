import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
    id: pane
    spacing: 12
    property int offset: 0
    function reload() { backend.searchHistory(search.text, favorites.checked, offset) }
    RowLayout {
        Layout.fillWidth: true
        TextField {
            id: search
            Layout.fillWidth: true
            placeholderText: qsTr("搜索历史记录")
            Accessible.name: qsTr("搜索历史记录")
            onTextEdited: { pane.offset = 0; debounce.restart() }
        }
        CheckBox { id: favorites; text: qsTr("仅收藏"); onToggled: { pane.offset = 0; pane.reload() } }
    }
    Timer { id: debounce; interval: 300; onTriggered: pane.reload() }
    Label { text: qsTr("普通历史默认关闭，敏感内容不会进入普通历史。"); wrapMode: Text.Wrap; Layout.fillWidth: true; opacity: 0.7 }
    Label { visible: backend.history.length === 0; text: qsTr("暂无历史记录"); Layout.fillWidth: true; horizontalAlignment: Text.AlignHCenter; Layout.topMargin: 16; Layout.bottomMargin: 16 }
    Repeater {
        model: backend.history
        delegate: Frame {
            required property var modelData
            Layout.fillWidth: true
            ColumnLayout {
                anchors.fill: parent
                spacing: 6
                Label { text: modelData.preview; textFormat: Text.PlainText; wrapMode: Text.WrapAnywhere; Layout.fillWidth: true; maximumLineCount: 5; elide: Text.ElideRight }
                Label { text: backend.receivedTime(modelData.timestamp); opacity: 0.65; font.pixelSize: 12 * backend.textScale }
                RowLayout {
                    Button { flat: true; text: qsTr("复制到剪贴板"); enabled: !backend.busy; onClicked: backend.invoke("copyHistory", {id: modelData.id}) }
                    Button { flat: true; text: modelData.favorite ? qsTr("取消收藏") : qsTr("收藏"); enabled: !backend.busy; onClicked: backend.invoke("setFavorite", {id: modelData.id, favorite: !modelData.favorite}) }
                    Button { flat: true; text: qsTr("删除"); enabled: !backend.busy; onClicked: backend.invoke("deleteHistory", {id: modelData.id}) }
                }
            }
        }
    }
    RowLayout {
        Layout.fillWidth: true
        Button { text: qsTr("上一页"); enabled: pane.offset > 0 && !backend.busy; onClicked: { pane.offset -= 20; pane.reload() } }
        Label { Layout.fillWidth: true; horizontalAlignment: Text.AlignHCenter; text: qsTr("共 %1 条").arg(backend.historyTotal) }
        Button { text: qsTr("下一页"); enabled: pane.offset + 20 < backend.historyTotal && !backend.busy; onClicked: { pane.offset += 20; pane.reload() } }
        Button { flat: true; text: qsTr("清空"); enabled: !backend.busy && backend.historyTotal > 0; onClicked: clearDialog.open() }
    }
    Dialog {
        id: clearDialog
        title: qsTr("清空本机普通历史？")
        implicitWidth: 420
        width: Math.min(420, pane.width)
        modal: true
        anchors.centerIn: Overlay.overlay
        standardButtons: Dialog.Ok | Dialog.Cancel
        onAccepted: backend.invoke("clearHistory")
        Label { text: qsTr("收藏也会删除，不影响服务端或接收游标。"); wrapMode: Text.Wrap }
    }
}

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ApplicationWindow {
    id: mainWindow
    objectName: "mainWindow"
    title: "RustClipSync"
    width: initialWidth
    height: 780
    minimumWidth: 480
    minimumHeight: 520
    visible: initialAction === "home"
    font.pixelSize: 14 * backend.textScale
    SystemPalette { id: systemColors; colorGroup: SystemPalette.Active }
    property bool dark: backend.theme === "dark" || (backend.theme === "system" && systemColors.window.hslLightness < 0.5)
    palette.window: dark ? "#242424" : "#f5f5f5"
    palette.windowText: dark ? "#eeeeee" : "#202020"
    palette.base: dark ? "#303030" : "#ffffff"
    palette.alternateBase: dark ? "#292929" : "#eeeeee"
    palette.text: dark ? "#eeeeee" : "#202020"
    palette.button: dark ? "#363636" : "#e9e9e9"
    palette.buttonText: dark ? "#eeeeee" : "#202020"
    palette.highlight: dark ? "#8ecbde" : "#176781"
    palette.highlightedText: dark ? "#172126" : "#ffffff"
    palette.placeholderText: dark ? "#b5b5b5" : "#606060"
    property var config: backend.status.config || ({})
    property var inbox: backend.status.inbox || ({})
    property bool activationOnly: initialAction !== "home"
    function exitIfIdle() { if (activationOnly && !sensitive.visible && !historyWindow.visible) backend.exitUi() }
    property bool formInitialized: false
    function loadConnectionForm() {
        if (!online || formInitialized) return
        serverField.text = config.server || ""
        accountField.text = config.user || ""
        const filter = config.filter || ({})
        filterMode.currentIndex = filter.mode === "blacklist" ? 1 : filter.mode === "whitelist" ? 2 : 0
        patterns.text = (filter.patterns || []).join("\n")
        filterDownloads.checked = !!filter.downloads
        formInitialized = true
    }
    Connections {
        target: backend
        function onChanged() { mainWindow.loadConnectionForm() }
        function onPaired() { mainWindow.formInitialized = false }
    }
    property bool online: !!backend.status.config
    function updateConfig(changes) {
        if (!online || backend.busy) return
        const next = JSON.parse(JSON.stringify(config))
        for (const key in changes) next[key] = changes[key]
        backend.saveSettings(next)
    }
    function filterSettings() { return {mode: ["off", "blacklist", "whitelist"][filterMode.currentIndex], patterns: patterns.text.split("\n").filter(function(p) { return p.length > 0 }), downloads: filterDownloads.checked} }
    function historySettings(changes) {
        const value = JSON.parse(JSON.stringify(config.history || ({enabled: false, days: 7, limit: 50})))
        for (const key in changes) value[key] = changes[key]
        updateConfig({history: value})
    }
    function activate(action) {
        backend.hideInbox()
        if (action === "inbox") {
            if (sensitive.visible) sensitive.hide()
            else { sensitive.show(); sensitive.raise(); sensitive.requestActivate(); if (!activationOnly) mainWindow.showMinimized() }
        } else if (action === "history") {
            if (historyWindow.visible) historyWindow.hide()
            else { historyWindow.show(); historyWindow.raise(); historyWindow.requestActivate(); historyPopup.reload() }
        } else { activationOnly = false; mainWindow.showNormal(); mainWindow.raise(); mainWindow.requestActivate() }
    }
    function smokeTest() {
        if (width < minimumWidth || tabs.count !== 4) return false
        if (initialPage === 0 && (!sensitiveBox.visible || sensitiveBox.width < 200)) return false
        for (let i = 0; i < 4; ++i) { tabs.currentIndex = i; if (!pages.itemAt(i)) return false }
        tabs.currentIndex = initialPage
        sensitive.show(); sensitive.hide()
        historyWindow.show(); historyWindow.hide()
        return !backend.inboxText
    }
    property int nativeStage: 0
    property int nativeRevision: 0
    function nativeTestStep() {
        if (backend.error) { console.error(backend.error); return -1 }
        if (backend.busy) return 0
        if (nativeStage === 0) {
            if (!online || !inbox.id) return 0
            if (backend.inboxText || !sensitiveBox.visible) return -1
            backend.theme = "dark"
            backend.textScale = 1.2
            nativeRevision = backend.status.configRevision
            updateConfig({history: {enabled: true, days: 14, limit: 50}})
            nativeStage = 1
        } else if (nativeStage === 1) {
            if (backend.status.configRevision <= nativeRevision) return 0
            if (config.history.days !== 14 || !config.paused || !config.allowSensitive || config.autoApplySensitive) return -1
            backend.revealInbox()
            nativeStage = 2
        } else if (nativeStage === 2) {
            if (!backend.inboxText) return 0
            if (backend.inboxText.indexOf("synthetic private fixture") !== 0) return -1
            backend.hideInbox()
            mainWindow.activate("inbox")
            if (!sensitive.visible || backend.inboxText) return -1
            sensitive.pinned = true
            if (!(sensitive.flags & Qt.WindowStaysOnTopHint)) return -1
            mainWindow.activate("inbox")
            if (sensitive.visible) return -1
            mainWindow.activate("home")
            mainWindow.activate("history")
            nativeStage = 3
        } else if (nativeStage === 3) {
            if (!historyWindow.visible || backend.historyTotal !== 1) return 0
            mainWindow.activate("history")
            if (historyWindow.visible) return -1
            backend.autostart = true
            if (!backend.autostart) return -1
            backend.autostart = false
            if (backend.autostart) return -1
            backend.theme = "light"
            backend.textScale = 1
            width = 480
            return 1
        }
        return 0
    }
    onActiveChanged: if (!active && !sensitive.active) backend.hideInbox()
    onClosing: { backend.hideInbox(); sensitive.hide(); historyWindow.hide(); backend.exitUi() }

    header: ColumnLayout {
        spacing: 8
        RowLayout {
            Layout.fillWidth: true
            Layout.topMargin: 20
            Layout.leftMargin: 24
            Layout.rightMargin: 24
            Image { source: "qrc:/assets/rustclipsync.png"; sourceSize.width: 32; sourceSize.height: 32; Layout.preferredWidth: 32; Layout.preferredHeight: 32 }
            Label { text: "RustClipSync"; font.pixelSize: 23 * backend.textScale; font.bold: true; Layout.fillWidth: true }
            BusyIndicator { running: backend.busy; visible: running; Layout.preferredWidth: 24; Layout.preferredHeight: 24 }
        }
        TabBar {
            id: tabs
            objectName: "navigation"
            Layout.alignment: Qt.AlignHCenter
            Layout.topMargin: 8
            Layout.bottomMargin: 4
            Layout.maximumWidth: mainWindow.width - 32
            currentIndex: initialPage
            onCurrentIndexChanged: backend.hideInbox()
            TabButton { text: qsTr("主页"); width: Math.min(110, (mainWindow.width - 32) / 4) }
            TabButton { text: qsTr("外观"); width: Math.min(110, (mainWindow.width - 32) / 4) }
            TabButton { text: qsTr("快捷键"); width: Math.min(110, (mainWindow.width - 32) / 4) }
            TabButton { text: qsTr("设置"); width: Math.min(110, (mainWindow.width - 32) / 4) }
        }
    }
    ColumnLayout {
        anchors.fill: parent
        spacing: 0
        Frame {
            visible: !!backend.error
            Layout.fillWidth: true
            Layout.margins: 16
            RowLayout {
                anchors.fill: parent
                Label { text: backend.error; textFormat: Text.PlainText; wrapMode: Text.WrapAnywhere; Layout.fillWidth: true }
                Button { text: qsTr("启动后台"); enabled: backend.configured && !backend.busy; onClicked: backend.startAgent() }
            }
        }
        StackLayout {
            id: pages
            currentIndex: tabs.currentIndex
            Layout.fillWidth: true
            Layout.fillHeight: true
            ScrollView {
                contentWidth: availableWidth
                clip: true
                ColumnLayout {
                    width: parent.width
                    spacing: 14
                    anchors.margins: 24
                    Label {
                        text: backend.configured ? backend.stateLabel(backend.status.state || "") : qsTr("连接你的剪贴板")
                        color: mainWindow.palette.highlight
                        font.pixelSize: 19 * backend.textScale
                        font.bold: true
                        Layout.fillWidth: true
                        Layout.leftMargin: 24; Layout.rightMargin: 24; Layout.topMargin: 20
                        wrapMode: Text.Wrap
                    }
                    Label { text: online ? qsTr("服务端：%1").arg(config.server) : qsTr("在设置中填写服务端、账户和密码，完成首次配对。"); textFormat: Text.PlainText; wrapMode: Text.WrapAnywhere; Layout.fillWidth: true; Layout.leftMargin: 24; Layout.rightMargin: 24 }
                    Button { visible: backend.configured && !online; text: qsTr("启动后台"); Layout.leftMargin: 24; onClicked: backend.startAgent() }
                    Label { visible: online; text: qsTr("设备：%1").arg(backend.status.deviceId || ""); textFormat: Text.PlainText; wrapMode: Text.WrapAnywhere; Layout.fillWidth: true; Layout.leftMargin: 24; Layout.rightMargin: 24; opacity: 0.7 }
                    Label { visible: online; text: qsTr("最近同步：%1%2").arg(backend.receivedTime(backend.status.lastSync || 0)).arg(backend.status.pending ? qsTr(" · 有待发送内容") : ""); wrapMode: Text.Wrap; Layout.fillWidth: true; Layout.leftMargin: 24; Layout.rightMargin: 24 }
                    RowLayout {
                        Layout.fillWidth: true; Layout.leftMargin: 24; Layout.rightMargin: 24
                        Button { text: qsTr("发送"); Layout.fillWidth: true; enabled: online && !backend.busy; onClicked: backend.invoke("pushNow") }
                        Button { text: qsTr("接收"); Layout.fillWidth: true; enabled: online && !backend.busy; onClicked: backend.invoke("pullNow") }
                        Button { text: config.paused ? qsTr("恢复") : qsTr("暂停"); Layout.fillWidth: true; enabled: online && !backend.busy; onClicked: backend.invoke("setPaused", {paused: !config.paused}) }
                    }
                    Label { text: qsTr("同步方向"); Layout.leftMargin: 24 }
                    ComboBox {
                        Layout.fillWidth: true; Layout.leftMargin: 24; Layout.rightMargin: 24
                        model: [qsTr("双向同步"), qsTr("仅发送"), qsTr("仅接收")]
                        currentIndex: config.direction === "upload" ? 1 : config.direction === "download" ? 2 : 0
                        enabled: online && !backend.busy
                        onActivated: updateConfig({direction: ["both", "upload", "download"][currentIndex]})
                    }
                    Frame {
                        id: sensitiveBox
                        objectName: "sensitiveInbox"
                        Layout.fillWidth: true; Layout.leftMargin: 24; Layout.rightMargin: 24
                        ColumnLayout {
                            anchors.fill: parent
                            spacing: 10
                            RowLayout {
                                Layout.fillWidth: true
                                Label { text: qsTr("敏感收件区"); font.bold: true; Layout.fillWidth: true }
                                Button { flat: true; text: qsTr("以小窗显示"); onClicked: mainWindow.activate("inbox") }
                            }
                            Label { text: !inbox.id ? qsTr("尚未收到敏感文本") : (backend.inboxText || "••••••••"); textFormat: Text.PlainText; wrapMode: Text.WrapAnywhere; Layout.fillWidth: true; maximumLineCount: 6; elide: Text.ElideRight }
                            Label { visible: !!inbox.id; text: qsTr("收到时间：%1").arg(backend.receivedTime(inbox.receivedAt || 0)); opacity: 0.7 }
                            Label { text: qsTr("只保留最新的手动收件项；自动写入模式的新内容不会存入此处。"); wrapMode: Text.Wrap; Layout.fillWidth: true; opacity: 0.7 }
                            RowLayout {
                                spacing: 0
                                Button { flat: true; text: qsTr("复制到剪贴板"); enabled: !!inbox.id && !backend.busy; onClicked: backend.invoke("copyInbox", {id: inbox.id}) }
                                Button { flat: true; text: backend.inboxText ? qsTr("隐藏") : qsTr("显示"); enabled: !!inbox.id && !backend.busy; onClicked: backend.revealInbox() }
                                Button { flat: true; text: qsTr("删除"); enabled: !!inbox.id && !backend.busy; onClicked: { backend.hideInbox(); backend.invoke("clearInbox", {id: inbox.id}) } }
                                Button { flat: true; text: qsTr("自动写入设置"); onClicked: tabs.currentIndex = 3 }
                            }
                        }
                    }
                    RowLayout {
                        Layout.leftMargin: 24; Layout.rightMargin: 24
                        Button { text: qsTr("连接设置"); onClicked: tabs.currentIndex = 3 }
                        Button { text: qsTr("退出后台"); enabled: online && !backend.busy; onClicked: quitDialog.open() }
                    }
                    CheckDelegate {
                        id: historyToggle
                        Layout.fillWidth: true; Layout.leftMargin: 24; Layout.rightMargin: 24
                        text: qsTr("历史记录")
                        onToggled: if (checked && online) historyPane.reload()
                    }
                    HistoryPane { id: historyPane; visible: historyToggle.checked; Layout.fillWidth: true; Layout.leftMargin: 24; Layout.rightMargin: 24; Layout.bottomMargin: 24 }
                    Item { Layout.preferredHeight: 12 }
                }
            }
            ScrollView {
                contentWidth: availableWidth
                ColumnLayout {
                    width: parent.width
                    spacing: 16
                    Label { text: qsTr("外观"); font.bold: true; font.pixelSize: 20 * backend.textScale; Layout.margins: 24 }
                    Label { text: qsTr("主题"); Layout.leftMargin: 24 }
                    ComboBox {
                        Layout.fillWidth: true; Layout.leftMargin: 24; Layout.rightMargin: 24
                        model: [qsTr("跟随系统"), qsTr("浅色"), qsTr("深色")]
                        currentIndex: backend.theme === "light" ? 1 : backend.theme === "dark" ? 2 : 0
                        onActivated: backend.theme = ["system", "light", "dark"][currentIndex]
                    }
                    Label { text: qsTr("字号"); Layout.leftMargin: 24 }
                    ComboBox {
                        Layout.fillWidth: true; Layout.leftMargin: 24; Layout.rightMargin: 24
                        model: [qsTr("标准"), qsTr("较大"), qsTr("大")]
                        currentIndex: backend.textScale > 1.3 ? 2 : backend.textScale > 1.1 ? 1 : 0
                        onActivated: backend.textScale = [1, 1.2, 1.4][currentIndex]
                    }
                }
            }
            ScrollView {
                contentWidth: availableWidth
                ColumnLayout {
                    width: parent.width
                    spacing: 16
                    Label { text: qsTr("快捷键"); font.bold: true; font.pixelSize: 20 * backend.textScale; Layout.margins: 24 }
                    Label { text: qsTr("在桌面环境的快捷键设置中绑定以下命令。再次触发会关闭对应小窗，关闭主界面后也可使用。"); wrapMode: Text.Wrap; Layout.fillWidth: true; Layout.leftMargin: 24; Layout.rightMargin: 24 }
                    Label { text: qsTr("快速打开历史记录"); Layout.leftMargin: 24 }
                    TextField { text: "rustclipsync --history"; readOnly: true; selectByMouse: true; Layout.fillWidth: true; Layout.leftMargin: 24; Layout.rightMargin: 24 }
                    Label { text: qsTr("以小窗打开敏感收件区"); Layout.leftMargin: 24 }
                    TextField { text: "rustclipsync --inbox"; readOnly: true; selectByMouse: true; Layout.fillWidth: true; Layout.leftMargin: 24; Layout.rightMargin: 24 }
                    Label { text: qsTr("组合键由桌面环境管理，不会捕获或记录键盘输入。"); wrapMode: Text.Wrap; Layout.fillWidth: true; Layout.leftMargin: 24; Layout.rightMargin: 24; opacity: 0.7 }
                }
            }
            ScrollView {
                contentWidth: availableWidth
                ColumnLayout {
                    width: parent.width
                    spacing: 12
                    Label { text: qsTr("连接设置"); font.bold: true; font.pixelSize: 20 * backend.textScale; Layout.margins: 24 }
                    Label { text: qsTr("服务端地址"); Layout.leftMargin: 24 }
                    TextField { id: serverField; placeholderText: "https://sync.example.com"; Layout.fillWidth: true; Layout.leftMargin: 24; Layout.rightMargin: 24 }
                    Label { text: qsTr("账户"); Layout.leftMargin: 24 }
                    TextField { id: accountField; Layout.fillWidth: true; Layout.leftMargin: 24; Layout.rightMargin: 24 }
                    Label { text: qsTr("密码"); Layout.leftMargin: 24 }
                    TextField { id: passwordField; echoMode: TextInput.Password; Layout.fillWidth: true; Layout.leftMargin: 24; Layout.rightMargin: 24 }
                    CheckBox { id: resetPairing; visible: backend.configured; text: qsTr("重新配对，替换现有身份与待发送内容"); Layout.leftMargin: 24 }
                    Button {
                        text: backend.busy ? qsTr("正在处理…") : qsTr("连接并配对")
                        Layout.leftMargin: 24
                        enabled: !backend.busy && serverField.text.length > 0 && accountField.text.length > 0 && passwordField.text.length > 0 && (!backend.configured || resetPairing.checked)
                        onClicked: { backend.pair(serverField.text, accountField.text, passwordField.text, resetPairing.checked); passwordField.clear() }
                    }
                    Label { text: qsTr("同步与隐私"); font.bold: true; Layout.leftMargin: 24; Layout.topMargin: 16 }
                    Switch { text: qsTr("同步已标记的敏感文本"); checked: !!config.allowSensitive; enabled: online && !backend.busy; Layout.leftMargin: 24; onClicked: updateConfig({allowSensitive: checked}) }
                    Label { text: qsTr("默认关闭。部分应用可能不标记敏感内容，因此不能识别所有敏感信息。"); wrapMode: Text.Wrap; Layout.fillWidth: true; Layout.leftMargin: 24; Layout.rightMargin: 24; opacity: 0.7 }
                    Switch { text: qsTr("收到敏感文本后自动写入系统剪贴板"); checked: !!config.autoApplySensitive; enabled: online && !!config.allowSensitive && !backend.busy; Layout.leftMargin: 24; onClicked: updateConfig({autoApplySensitive: checked}) }
                    Label { text: qsTr("默认关闭；关闭时进入加密敏感收件区。自动写入仅影响新接收内容，不会补写旧收件项。"); wrapMode: Text.Wrap; Layout.fillWidth: true; Layout.leftMargin: 24; Layout.rightMargin: 24; opacity: 0.7 }
                    Label { text: qsTr("历史记录"); font.bold: true; Layout.leftMargin: 24; Layout.topMargin: 16 }
                    Switch { text: qsTr("保存加密普通历史"); checked: !!(config.history || {}).enabled; enabled: online && !backend.busy; Layout.leftMargin: 24; onClicked: historySettings({enabled: checked}) }
                    RowLayout {
                        Layout.leftMargin: 24; Layout.rightMargin: 24
                        Label { text: qsTr("保留天数") }
                        SpinBox { from: 1; to: 365; value: (config.history || {}).days || 7; enabled: online && !backend.busy; onValueModified: historySettings({days: value}) }
                        Label { text: qsTr("条数") }
                        SpinBox { from: 1; to: 1000; value: (config.history || {}).limit || 50; enabled: online && !backend.busy; onValueModified: historySettings({limit: value}) }
                    }
                    Label { text: qsTr("敏感正文不进入普通历史；删除仅影响本机，不重置接收游标。"); wrapMode: Text.Wrap; Layout.fillWidth: true; Layout.leftMargin: 24; Layout.rightMargin: 24; opacity: 0.7 }
                    Label { text: qsTr("正则过滤"); font.bold: true; Layout.leftMargin: 24; Layout.topMargin: 16 }
                    ComboBox { id: filterMode; model: [qsTr("关闭"), qsTr("黑名单"), qsTr("白名单")]; Layout.fillWidth: true; Layout.leftMargin: 24; Layout.rightMargin: 24 }
                    ScrollView {
                        Layout.fillWidth: true; Layout.preferredHeight: 100; Layout.leftMargin: 24; Layout.rightMargin: 24
                        TextArea { id: patterns; placeholderText: qsTr("每行一个表达式，最多 32 条"); wrapMode: TextEdit.WrapAnywhere }
                    }
                    CheckBox { id: filterDownloads; text: qsTr("同时过滤接收内容"); Layout.leftMargin: 24 }
                    Button { text: qsTr("保存过滤规则"); enabled: online && !backend.busy; Layout.leftMargin: 24; onClicked: updateConfig({filter: filterSettings()}) }
                    TextField { id: filterSample; placeholderText: qsTr("输入非敏感样例测试过滤规则"); Layout.fillWidth: true; Layout.leftMargin: 24; Layout.rightMargin: 24; maximumLength: 8192 }
                    Button { text: qsTr("测试过滤规则"); enabled: online && !backend.busy; Layout.leftMargin: 24; onClicked: backend.testFilter(filterSettings(), filterSample.text) }
                    Label { text: backend.filterResult; visible: text.length > 0; wrapMode: Text.Wrap; Layout.fillWidth: true; Layout.leftMargin: 24; Layout.rightMargin: 24 }
                    RowLayout {
                        Layout.leftMargin: 24; Layout.topMargin: 12
                        Label { text: qsTr("备用检查间隔（秒）") }
                        SpinBox { from: 30; to: 300; value: config.intervalSec || 60; enabled: online && !backend.busy; onValueModified: updateConfig({intervalSec: value}) }
                    }
                    Switch { text: qsTr("登录后启动同步后台"); checked: backend.autostart; Layout.leftMargin: 24; onClicked: backend.autostart = checked }
                    Label { text: qsTr("关于"); font.bold: true; Layout.leftMargin: 24; Layout.topMargin: 16 }
                    Label { text: qsTr("RustClipSync Desktop · Qt 6/QML + Rust\n状态与历史在本机加密，密钥保存在桌面密钥环。HTTPS 保护传输，服务端仍可读取同步内容。"); wrapMode: Text.Wrap; Layout.fillWidth: true; Layout.leftMargin: 24; Layout.rightMargin: 24 }
                    Button { flat: true; text: qsTr("项目与使用说明"); Layout.leftMargin: 24; onClicked: Qt.openUrlExternally("https://github.com/Infinifar/rustclipsync-desktop") }
                    Item { Layout.preferredHeight: 24 }
                }
            }
        }
    }
    SensitiveWindow { id: sensitive; parentPalette: mainWindow.palette; onVisibleChanged: { if (!visible) { backend.hideInbox(); Qt.callLater(mainWindow.exitIfIdle) } } }
    ApplicationWindow {
        id: historyWindow
        title: qsTr("历史记录")
        width: 600; height: 640; minimumWidth: 420; minimumHeight: 360
        visible: false
        palette: mainWindow.palette
        font: mainWindow.font
        onVisibleChanged: if (!visible) Qt.callLater(mainWindow.exitIfIdle)
        onClosing: function(close) { close.accepted = false; hide() }
        ScrollView {
            anchors.fill: parent; anchors.margins: 20
            contentWidth: availableWidth
            HistoryPane { id: historyPopup; width: parent.width }
        }
    }
    Dialog {
        id: quitDialog
        title: qsTr("退出同步后台？")
        implicitWidth: 420
        width: Math.min(420, mainWindow.width - 48)
        anchors.centerIn: Overlay.overlay
        modal: true
        standardButtons: Dialog.Ok | Dialog.Cancel
        Label { text: qsTr("退出后将停止同步，可以重新打开应用启动。"); wrapMode: Text.Wrap }
        onAccepted: backend.invoke("quit")
    }
}

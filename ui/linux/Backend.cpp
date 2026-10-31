#include "Backend.h"
#include <QCoreApplication>
#include <QCryptographicHash>
#include <QDateTime>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonDocument>
#include <QJsonArray>
#include <QLocale>
#include <memory>
#include <QLocalSocket>
#include <QProcess>
#include <QSaveFile>
#include <QStandardPaths>
#include <QUuid>
#include <QtEndian>
#include <sys/socket.h>
#include <sys/file.h>
#include <fcntl.h>
#include <unistd.h>

namespace { constexpr quint32 MaxFrame = 256 * 1024; }
Backend::Backend(QString directory, bool smoke, QObject *parent)
    : QObject(parent), m_directory(std::move(directory)), m_smoke(smoke),
      m_settings(m_directory + "/ui.ini", QSettings::IniFormat) {
    m_poll.setInterval(1500);
    connect(&m_poll, &QTimer::timeout, this, &Backend::refresh);
    if (!smoke) {
        m_poll.start();
        QTimer::singleShot(0, this, [this] { if (configured()) startAgent(); });
    }
}
bool Backend::configured() const { return QFileInfo::exists(m_directory + "/config.json"); }
QString Backend::theme() const { return m_settings.value("appearance/theme", "system").toString(); }
double Backend::textScale() const { return m_settings.value("appearance/textScale", 1.0).toDouble(); }
void Backend::setTheme(const QString &value) {
    if (value != "system" && value != "light" && value != "dark") return;
    m_settings.setValue("appearance/theme", value); emit appearanceChanged();
}
void Backend::setTextScale(double value) {
    if (value < 1 || value > 1.5) return;
    m_settings.setValue("appearance/textScale", value); emit appearanceChanged();
}
QString Backend::agentPath() const {
    const auto base = QCoreApplication::applicationDirPath();
    for (const auto &path : {base + "/rustclipsync-agent", base + "/../libexec/rustclipsync/rustclipsync-agent"})
        if (QFileInfo(path).isExecutable()) return QDir::cleanPath(path);
    return QStandardPaths::findExecutable("rustclipsync-agent");
}
QStringList Backend::agentArguments(const QStringList &args) const {
    return QStringList{"--data-dir", m_directory} + args;
}
QString Backend::socketPath() const {
    const auto fingerprint = QCryptographicHash::hash(m_directory.toUtf8(), QCryptographicHash::Sha256).toHex().toUpper().left(24);
    return qEnvironmentVariable("XDG_RUNTIME_DIR") + "/rustclipsync-desktop/" + QString::fromLatin1(fingerprint) + ".sock";
}
void Backend::fail(const QString &message) { m_error = message; emit changed(); }
void Backend::rpc(const QString &method, const QJsonObject &params, Reply reply, bool quiet) {
    if (m_smoke) return;
    if (!quiet && m_busy) { fail("请等待当前操作完成。"); return; }
    if (!quiet) { m_busy = true; m_error.clear(); emit changed(); }
    auto socket = new QLocalSocket(this);
    auto timer = new QTimer(socket);
    timer->setSingleShot(true); timer->start(65000);
    auto buffer = std::make_shared<QByteArray>();
    auto done = std::make_shared<bool>(false);
    const auto id = QUuid::createUuid().toString(QUuid::WithoutBraces);
    auto finish = [this, socket, done, quiet](const QString &error) {
        if (*done) return;
        *done = true;
        if (quiet) m_polling = false; else m_busy = false;
        if (!error.isEmpty()) m_error = error;
        emit changed();
        socket->abort(); socket->deleteLater();
    };
    connect(timer, &QTimer::timeout, socket, [finish] { finish("后台操作超时，请检查连接后重试。"); });
    connect(socket, &QLocalSocket::errorOccurred, socket, [this, finish, quiet](QLocalSocket::LocalSocketError) {
        if (quiet) m_status.clear();
        finish(quiet ? QString() : QStringLiteral("无法连接后台，请点击“启动后台”并检查桌面会话与密钥环。"));
    });
    connect(socket, &QLocalSocket::connected, socket, [socket, method, params, id, finish] {
        struct ucred peer{}; socklen_t size = sizeof(peer);
        if (getsockopt(socket->socketDescriptor(), SOL_SOCKET, SO_PEERCRED, &peer, &size) != 0 || peer.uid != geteuid()) {
            finish("后台连接身份验证失败。"); return;
        }
        const auto bytes = QJsonDocument(QJsonObject{{"id", id}, {"ipcVersion", 1}, {"method", method}, {"params", params}}).toJson(QJsonDocument::Compact);
        if (bytes.isEmpty() || bytes.size() > MaxFrame) { finish("请求超出大小限制。"); return; }
        QByteArray header(4, '\0'); qToLittleEndian<quint32>(bytes.size(), header.data());
        socket->write(header); socket->write(bytes);
    });
    connect(socket, &QLocalSocket::readyRead, socket, [this, socket, buffer, done, finish, id, reply] {
        if (*done) return;
        buffer->append(socket->readAll());
        if (buffer->size() > MaxFrame + 4) { finish("后台响应超出大小限制。"); return; }
        if (buffer->size() < 4) return;
        const auto size = qFromLittleEndian<quint32>(buffer->constData());
        if (size == 0 || size > MaxFrame) { finish("后台响应格式无效。"); return; }
        if (buffer->size() < size + 4) return;
        if (buffer->size() != size + 4) { finish("后台响应格式无效。"); return; }
        QJsonParseError error;
        const auto object = QJsonDocument::fromJson(buffer->mid(4, size), &error).object();
        if (error.error != QJsonParseError::NoError || object.value("id").toString() != id) { finish("后台响应身份无效。"); return; }
        if (!object.value("ok").toBool()) { finish(stateLabel(object.value("error").toString())); return; }
        finish({}); reply(object.value("result").toObject());
    });
    socket->connectToServer(socketPath());
}
void Backend::refresh() {
    if (!configured() || m_busy || m_polling || m_smoke) return;
    m_polling = true;
    rpc("getStatus", {}, [this](const QJsonObject &result) {
        const auto previousId = m_status.value("inbox").toMap().value("id").toString();
        const auto historyRevision = m_status.value("historyRevision");
        m_status = result.toVariantMap();
        if (m_historyRequested && historyRevision != m_status.value("historyRevision")) searchHistory(m_query, m_favorites, m_offset);
        if (previousId != m_status.value("inbox").toMap().value("id").toString()) hideInbox();
        emit changed();
    }, true);
}
void Backend::startAgent() {
    if (!configured()) { fail("请先在设置中连接服务端。"); return; }
    // Try IPC first; the profile lock handles concurrent launches safely.
    auto probe = new QLocalSocket(this);
    connect(probe, &QLocalSocket::connected, probe, [this, probe] { probe->deleteLater(); refresh(); });
    connect(probe, &QLocalSocket::errorOccurred, probe, [this, probe](QLocalSocket::LocalSocketError) {
        probe->deleteLater();
        if (agentPath().isEmpty()) { fail("找不到 rustclipsync-agent，请检查安装包。"); return; }
        QProcess agent;
        agent.setProgram(agentPath()); agent.setArguments(agentArguments({"run"}));
        agent.setStandardOutputFile(QProcess::nullDevice());
        agent.setStandardErrorFile(m_directory + "/agent-startup.log");
        if (!agent.startDetached()) { fail("后台启动失败。"); return; }
        QTimer::singleShot(800, this, [this] {
            QFile log(m_directory + "/agent-startup.log");
            if (log.open(QIODevice::ReadOnly) && log.size() > 0) fail(QString::fromUtf8(log.read(2048)).trimmed());
            refresh();
        });
    });
    probe->connectToServer(socketPath());
}
void Backend::invoke(const QString &method, const QVariantMap &params) {
    rpc(method, QJsonObject::fromVariantMap(params), [this, method](const QJsonObject &) {
        if (method == "quit") { m_status.clear(); emit changed(); emit quitRequested(); return; }
        if (method == "clearInbox") hideInbox();
        refresh();
        if (method.contains("History") || method == "setFavorite") searchHistory(m_query, m_favorites, m_offset);
    });
}
void Backend::saveSettings(const QVariantMap &config) {
    rpc("updateSettings", {{"config", QJsonObject::fromVariantMap(config)}, {"expectedRevision", m_status.value("configRevision").toDouble()}}, [this](const QJsonObject &) { refresh(); });
}
void Backend::pair(const QString &server, const QString &account, const QString &password, bool reset) {
    if (m_busy || password.isEmpty() || agentPath().isEmpty()) { fail("请填写服务端、账户和密码，并检查后台组件。"); return; }
    if (!reset || !configured()) { beginPair(server, account, password, reset); return; }
    auto probe = new QLocalSocket(this);
    connect(probe, &QLocalSocket::connected, probe, [this, probe, server, account, password, reset] {
        probe->deleteLater();
        rpc("quit", {}, [this, server, account, password, reset](const QJsonObject &) {
            // The long-poll worker may need its current timeout to stop. Wait
            // for the actual profile lock instead of racing a fixed delay.
            m_busy = true; emit changed();
            auto waiter = new QTimer(this);
            waiter->setInterval(100);
            connect(waiter, &QTimer::timeout, waiter, [this, waiter, server, account, password, reset] {
                const auto path = QFile::encodeName(m_directory + "/agent.lock");
                const int file = open(path.constData(), O_RDWR | O_NOFOLLOW | O_CLOEXEC);
                const bool available = file >= 0 && flock(file, LOCK_EX | LOCK_NB) == 0;
                if (file >= 0) close(file);
                if (!available) return;
                waiter->stop(); waiter->deleteLater(); beginPair(server, account, password, reset);
            });
            QTimer::singleShot(65000, waiter, [this, waiter] { waiter->stop(); waiter->deleteLater(); m_busy = false; fail("后台尚未停止，请稍后重新配对。"); });
            waiter->start();
        });
    });
    connect(probe, &QLocalSocket::errorOccurred, probe, [this, probe, server, account, password, reset](QLocalSocket::LocalSocketError) {
        probe->deleteLater(); beginPair(server, account, password, reset);
    });
    probe->connectToServer(socketPath());
}
void Backend::beginPair(const QString &server, const QString &account, const QString &password, bool reset) {
    // No password in arguments, environment, QSettings, diagnostics or IPC.
    m_busy = true; m_error.clear(); emit changed();
    auto process = new QProcess(this);
    auto args = QStringList{"pair", server.trimmed(), account, "--password-stdin"};
    if (reset) args << "--reset";
    process->setProgram(agentPath()); process->setArguments(agentArguments(args));
    auto secret = std::make_shared<QByteArray>(password.toUtf8());
    connect(process, &QProcess::started, process, [process, secret] { process->write(*secret); process->closeWriteChannel(); secret->fill('\0'); secret->clear(); });
    connect(process, &QProcess::errorOccurred, process, [this, process, secret](QProcess::ProcessError) {
        secret->fill('\0'); secret->clear(); m_busy = false; fail("配对进程启动或运行失败。"); process->deleteLater();
    });
    connect(process, qOverload<int, QProcess::ExitStatus>(&QProcess::finished), process, [this, process](int code, QProcess::ExitStatus status) {
        m_busy = false;
        if (code != 0 || status != QProcess::NormalExit) { fail(QString::fromUtf8(process->readAllStandardError().left(2048)).trimmed()); }
        else { m_status.clear(); emit changed(); emit paired(); startAgent(); }
        process->deleteLater();
    });
    QTimer::singleShot(65000, process, [process] { if (process->state() != QProcess::NotRunning) process->kill(); });
    process->start();
}
void Backend::searchHistory(const QString &query, bool favorites, int offset) {
    m_historyRequested = true;
    m_query = query; m_favorites = favorites; m_offset = qMax(0, offset);
    rpc("listHistory", {{"query", query}, {"favorites", favorites}, {"offset", m_offset}}, [this](const QJsonObject &result) {
        m_history = result.value("items").toArray().toVariantList(); m_total = result.value("total").toInt(); emit historyChanged();
    });
}
void Backend::hideInbox() { m_inboxText.clear(); m_revealedId.clear(); emit inboxTextChanged(); }
void Backend::revealInbox() {
    const auto id = m_status.value("inbox").toMap().value("id").toString();
    if (id.isEmpty()) return;
    if (!m_inboxText.isEmpty()) { hideInbox(); return; }
    m_revealedId = id; previewChunk(id, 0, 0, {});
}
void Backend::previewChunk(const QString &id, int offset, int count, const QString &text) {
    rpc("readInbox", {{"id", id}, {"offset", offset}}, [this, id, offset, count, text](const QJsonObject &result) {
        if (m_revealedId != id) return;
        const auto chunk = result.value("text").toString();
        const auto next = result.value("nextOffset").toInt(-1), total = result.value("total").toInt(-1);
        if (result.value("id").toString() != id || result.value("offset").toInt(-1) != offset || next < offset || next > total || total > 10 * 1024 * 1024 || next - offset != chunk.toUtf8().size() || next - offset > 16 * 1024 || (next == offset && next != total)) {
            hideInbox(); fail("敏感收件内容已改变，请重新打开。"); return;
        }
        const auto combined = text + chunk;
        if (next < total && count < 3) { previewChunk(id, next, count + 1, combined); return; }
        rpc("getStatus", {}, [this, id, next, total, combined](const QJsonObject &status) {
            if (m_revealedId != id || status.value("inbox").toObject().value("id").toString() != id) { hideInbox(); return; }
            m_inboxText = combined + (next < total ? QStringLiteral("\n…预览已截断；复制按钮仍复制完整原文。") : QString()); emit inboxTextChanged();
        });
    });
}
QString Backend::startupPath() const {
    const auto base = qEnvironmentVariable("XDG_CONFIG_HOME", QDir::homePath() + "/.config");
    return base + "/autostart/io.github.infinifar.rustclipsync.desktop";
}
bool Backend::autostart() const { return QFileInfo::exists(startupPath()); }
void Backend::setAutostart(bool enabled) {
    if (m_smoke) return;
    if (!enabled) { if (autostart() && !QFile::remove(startupPath())) fail("无法删除登录自启入口。"); emit changed(); return; }
    if (!QDir().mkpath(QFileInfo(startupPath()).absolutePath())) { fail("无法创建登录自启目录。"); return; }
    auto quote = [](QString s) { s.replace("\\", "\\\\").replace("\"", "\\\"").replace("`", "\\`").replace("$", "\\$").replace("%", "%%"); return '"' + s + '"'; };
    QSaveFile file(startupPath());
    const auto entry = QString("[Desktop Entry]\nType=Application\nName=RustClipSync\nExec=%1 --data-dir %2 autostart\nIcon=io.github.infinifar.rustclipsync\nTerminal=false\n").arg(quote(agentPath()), quote(m_directory));
    if (!file.open(QIODevice::WriteOnly) || file.write(entry.toUtf8()) != entry.toUtf8().size() || !file.commit()) fail("无法保存登录自启入口。");
    emit changed();
}
QString Backend::receivedTime(qint64 seconds) const { return seconds > 0 ? QLocale().toString(QDateTime::fromSecsSinceEpoch(seconds), QLocale::ShortFormat) : QStringLiteral("尚未同步"); }
QString Backend::stateLabel(const QString &state) const {
    static const QHash<QString, QString> labels = {{"connected", "已连接"}, {"connecting", "正在连接"}, {"paused", "已暂停自动同步"}, {"offline", "网络连接失败，将自动重试"}, {"credentials", "凭据不正确，请重新配对"}, {"conflict", "存在同步冲突，请明确发送或接收"}, {"expired", "待发送内容已过期，请重新复制"}, {"re_pair", "服务端状态已改变，需要重新配对"}, {"storage_error", "加密存储不可用，请检查密钥环与文件权限"}, {"clipboard_error", "剪贴板访问失败，请检查桌面支持"}, {"settings_changed", "设置已改变，请刷新后重试"}, {"history_error", "历史记录不可用"}, {"invalid_request", "操作已失效，请刷新后重试"}, {"needs_restart", "后台需要重新启动"}};
    return labels.value(state, state.isEmpty() ? QStringLiteral("后台未连接") : state);
}

void Backend::testFilter(const QVariantMap &filter, const QString &text) {
    rpc("testFilter", {{"filter", QJsonObject::fromVariantMap(filter)}, {"text", text}}, [this](const QJsonObject &result) {
        m_filterResult = QString("%1 · 发送：%2 · 接收：%3").arg(result.value("matched").toBool() ? "匹配" : "未匹配", result.value("uploadAllowed").toBool() ? "允许" : "过滤", result.value("downloadAllowed").toBool() ? "允许" : "过滤");
        emit changed();
    });
}

void Backend::exitUi() { hideInbox(); QCoreApplication::exit(0); }

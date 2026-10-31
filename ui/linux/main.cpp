#include "Backend.h"
#include <QGuiApplication>
#include <QIcon>
#include <QCommandLineParser>
#include <QCryptographicHash>
#include <QDir>
#include <QFile>
#include <QJsonObject>
#include <QLocalServer>
#include <QLockFile>
#include <QLocalSocket>
#include <QQmlApplicationEngine>
#include <QQmlContext>
#include <QQuickStyle>
#include <QQuickWindow>
#include <QStandardPaths>
#include <QTimer>
#include <sys/socket.h>
#include <sys/stat.h>
#include <unistd.h>

static bool privateDirectory(const QString &path) {
    if (!QDir::isAbsolutePath(path) || !QDir().mkpath(path)) return false;
    struct stat st{};
    const auto bytes = QFile::encodeName(path);
    return lstat(bytes.constData(), &st) == 0 && S_ISDIR(st.st_mode) && st.st_uid == geteuid() && chmod(bytes.constData(), 0700) == 0;
}
int main(int argc, char **argv) {
    umask(0077);
    // Keep system Qt Quick controls; no browser engine or desktop shell dependency.
    QQuickStyle::setStyle("Fusion");
    QGuiApplication app(argc, argv);
    app.setApplicationName("rustclipsync"); app.setOrganizationName("Infinifar"); app.setApplicationVersion("0.2.3");
    app.setWindowIcon(QIcon("qrc:/assets/rustclipsync.png"));
    QCommandLineParser parser; parser.addHelpOption(); parser.addVersionOption();
    parser.addOptions({{"history", "Toggle the history window"}, {"inbox", "Toggle the masked sensitive inbox"}, {"data-dir", "Use an isolated absolute profile directory", "directory"}, {"smoke-test", "Run isolated QML checks without starting a background agent"}, {"integration-test", "Verify commands against a disposable native agent"}, {"capture", "Save an isolated UI verification screenshot", "file"}, {"page", "Initial page for UI verification (0-3)", "number", "0"}, {"width", "Initial window width", "pixels", "760"}});
    parser.process(app);
    const auto directory = QDir::cleanPath(parser.isSet("data-dir") ? parser.value("data-dir") : qEnvironmentVariable("XDG_STATE_HOME", QDir::homePath() + "/.local/state") + "/rustclipsync-desktop");
    if (!privateDirectory(directory)) { qCritical("Invalid private state directory"); return 1; }
    const bool smoke = parser.isSet("smoke-test");
    const bool integration = parser.isSet("integration-test");
    if (integration && qEnvironmentVariable("RCS_LINUX_DISPOSABLE") != "1") { qCritical("Use the isolated native test runner"); return 1; }
    const auto runtime = qEnvironmentVariable("XDG_RUNTIME_DIR");
    const auto uiDir = runtime + "/rustclipsync-desktop";
    if (runtime.isEmpty() || !privateDirectory(uiDir)) { qCritical("A private XDG_RUNTIME_DIR is required"); return 1; }
    const auto fingerprint = QCryptographicHash::hash(directory.toUtf8(), QCryptographicHash::Sha256).toHex().toUpper().left(24);
    const auto socketName = uiDir + "/" + QString::fromLatin1(fingerprint) + ".ui";
    const auto action = parser.isSet("history") ? QStringLiteral("history") : parser.isSet("inbox") ? QStringLiteral("inbox") : QStringLiteral("home");
    QLocalServer server;
    QLockFile uiLock(directory + "/ui.lock");
    if (!smoke) {
        QLocalSocket existing; existing.connectToServer(socketName);
        if (existing.waitForConnected(250)) {
            existing.write(action.toUtf8() + '\n');
            existing.waitForBytesWritten(500);
            if (!existing.canReadLine() && !existing.waitForReadyRead(2500)) { qCritical("UI activation was not acknowledged"); return 1; }
            return existing.readLine() == "ok\n" ? 0 : 1;
        }
        if (!uiLock.tryLock(0)) { qCritical("Another UI instance is starting; retry activation"); return 1; }
        QLocalServer::removeServer(socketName);
        server.setSocketOptions(QLocalServer::UserAccessOption);
        if (!server.listen(socketName)) { qCritical("Cannot create UI activation socket"); return 1; }
    }
    Backend backend(directory, smoke);
    QQmlApplicationEngine engine;
    bool qmlWarnings = false;
    QObject::connect(&engine, &QQmlEngine::warnings, &app, [&qmlWarnings](const QList<QQmlError> &) { qmlWarnings = true; });
    engine.rootContext()->setContextProperty("backend", &backend);
    engine.rootContext()->setContextProperty("initialAction", action);
    engine.rootContext()->setContextProperty("initialPage", qBound(0, parser.value("page").toInt(), 3));
    engine.rootContext()->setContextProperty("initialWidth", qBound(480, parser.value("width").toInt(), 1600));
    engine.load(QUrl("qrc:/Main.qml"));
    if (engine.rootObjects().isEmpty()) return 1;
    auto root = engine.rootObjects().first();
    auto activate = [root](const QString &command) {
        const QVariant argument(command);
        QMetaObject::invokeMethod(root, "activate", Q_ARG(QVariant, argument));
    };
    QObject::connect(&server, &QLocalServer::newConnection, &app, [&server, activate] {
        while (auto socket = server.nextPendingConnection()) {
            struct ucred peer{}; socklen_t length = sizeof(peer);
            if (getsockopt(socket->socketDescriptor(), SOL_SOCKET, SO_PEERCRED, &peer, &length) != 0 || peer.uid != geteuid()) { socket->deleteLater(); continue; }
            const auto readActivation = [socket, activate] {
                if (socket->bytesAvailable() > 32) { socket->abort(); socket->deleteLater(); return; }
                if (!socket->canReadLine()) return;
                const auto command = QString::fromUtf8(socket->readLine(32)).trimmed();
                if (command == "home" || command == "history" || command == "inbox") {
                    activate(command); socket->write("ok\n"); socket->flush();
                }
                socket->disconnectFromServer(); socket->deleteLater();
            };
            QObject::connect(socket, &QLocalSocket::readyRead, socket, readActivation);
            if (socket->bytesAvailable()) readActivation();
            QTimer::singleShot(2000, socket, &QObject::deleteLater);
        }
    });
    QObject::connect(&backend, &Backend::quitRequested, &app, [] { QCoreApplication::exit(0); });
    if (!smoke) QTimer::singleShot(0, root, [activate, action] { activate(action); });
    if (smoke) QTimer::singleShot(1000, &app, [&app, root, &qmlWarnings] {
        QVariant result;
        const bool invoked = QMetaObject::invokeMethod(root, "smokeTest", Q_RETURN_ARG(QVariant, result));
        app.exit(invoked && result.toBool() && !qmlWarnings ? 0 : 2);
    });
    if (integration) {
        auto verification = new QTimer(&app);
        verification->setInterval(100);
        QObject::connect(verification, &QTimer::timeout, &app, [&app, root, verification, &qmlWarnings] {
            if (qmlWarnings) { app.exit(8); return; }
            QVariant result;
            if (!QMetaObject::invokeMethod(root, "nativeTestStep", Q_RETURN_ARG(QVariant, result))) { app.exit(4); return; }
            if (result.toInt() != 0) {
                verification->stop();
                if (result.toInt() != 1) { app.exit(5); return; }
                QTimer::singleShot(250, &app, [&app, root] {
                    const auto window = qobject_cast<QQuickWindow *>(root);
                    const auto capture = qEnvironmentVariable("RCS_LINUX_CAPTURE_DIR");
                    if (!capture.isEmpty()) {
                        QDir().mkpath(capture);
                        if (!window || !window->grabWindow().save(capture + "/native-paired-home.png")) { app.exit(7); return; }
                    }
                    // Exercise the real main-window close path. Hidden popups
                    // must not veto ending the UI, and the Rust agent survives.
                    if (!window || !window->close()) { app.exit(9); return; }
                    QTimer::singleShot(1000, &app, [&app] { app.exit(10); });
                });
            }
        });
        verification->start();
        QTimer::singleShot(25000, &app, [&app] { qCritical("Native UI verification timed out"); app.exit(6); });
    }
    if (parser.isSet("capture")) QTimer::singleShot(750, &app, [&app, root, &parser] {
        auto window = qobject_cast<QQuickWindow *>(root);
        if (!window || !window->grabWindow().save(parser.value("capture"))) app.exit(3);
    });
    return app.exec();
}

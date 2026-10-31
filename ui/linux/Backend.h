#pragma once
#include <QObject>
#include <QJsonObject>
#include <QSettings>
#include <QTimer>
#include <QVariantMap>
#include <functional>

class Backend : public QObject {
    Q_OBJECT
    Q_PROPERTY(QVariantMap status READ status NOTIFY changed)
    Q_PROPERTY(QVariantList history READ history NOTIFY historyChanged)
    Q_PROPERTY(int historyTotal READ historyTotal NOTIFY historyChanged)
    Q_PROPERTY(bool configured READ configured NOTIFY changed)
    Q_PROPERTY(bool busy READ busy NOTIFY changed)
    Q_PROPERTY(QString error READ error NOTIFY changed)
    Q_PROPERTY(QString inboxText READ inboxText NOTIFY inboxTextChanged)
    Q_PROPERTY(QString theme READ theme WRITE setTheme NOTIFY appearanceChanged)
    Q_PROPERTY(double textScale READ textScale WRITE setTextScale NOTIFY appearanceChanged)
    Q_PROPERTY(bool autostart READ autostart WRITE setAutostart NOTIFY changed)
public:
    explicit Backend(QString directory, bool smoke, QObject *parent = nullptr);
    QVariantMap status() const { return m_status; }
    QVariantList history() const { return m_history; }
    int historyTotal() const { return m_total; }
    bool configured() const;
    bool busy() const { return m_busy; }
    QString error() const { return m_error; }
    QString inboxText() const { return m_inboxText; }
    QString theme() const;
    double textScale() const;
    bool autostart() const;
    void setTheme(const QString &theme);
    void setTextScale(double scale);
    void setAutostart(bool enabled);
    Q_INVOKABLE void refresh();
    Q_INVOKABLE void startAgent();
    Q_INVOKABLE void invoke(const QString &method, const QVariantMap &params = {});
    Q_INVOKABLE void saveSettings(const QVariantMap &config);
    Q_INVOKABLE void pair(const QString &server, const QString &account, const QString &password, bool reset);
    Q_INVOKABLE void searchHistory(const QString &query = {}, bool favorites = false, int offset = 0);
    Q_INVOKABLE void revealInbox();
    Q_INVOKABLE void hideInbox();
    Q_INVOKABLE QString stateLabel(const QString &state) const;
    Q_INVOKABLE QString receivedTime(qint64 seconds) const;
    QString directory() const { return m_directory; }
signals:
    void changed();
    void historyChanged();
    void inboxTextChanged();
    void appearanceChanged();
    void paired();
    void quitRequested();
private:
    using Reply = std::function<void(const QJsonObject &)>;
    void rpc(const QString &method, const QJsonObject &params, Reply reply, bool quiet = false);
    void previewChunk(const QString &id, int offset, int count, const QString &text);
    void beginPair(const QString &server, const QString &account, const QString &password, bool reset);
    QString agentPath() const;
    QString socketPath() const;
    QString startupPath() const;
    QStringList agentArguments(const QStringList &args) const;
    void fail(const QString &message);
    QString m_directory, m_error, m_inboxText, m_revealedId;
    QVariantMap m_status;
    QVariantList m_history;
    int m_total = 0;
    bool m_busy = false, m_smoke = false, m_polling = false;
    QSettings m_settings;
    QTimer m_poll;
    QString m_query;
    bool m_favorites = false;
    int m_offset = 0;
};

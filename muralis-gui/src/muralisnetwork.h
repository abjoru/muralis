#ifndef MURALISNETWORK_H
#define MURALISNETWORK_H

#include <QNetworkAccessManager>
#include <QQmlNetworkAccessManagerFactory>

// Every request the QML engine issues — a drawer's full-resolution Image as
// much as anything else — goes out identified. Stamping it here rather than at
// each call site means a future Image cannot reintroduce an anonymous request.
class MuralisNetworkAccessManager : public QNetworkAccessManager {
    Q_OBJECT
public:
    explicit MuralisNetworkAccessManager(QObject *parent = nullptr);

protected:
    QNetworkReply *createRequest(Operation op, const QNetworkRequest &request,
                                 QIODevice *outgoingData = nullptr) override;
};

class MuralisNetworkAccessManagerFactory : public QQmlNetworkAccessManagerFactory {
public:
    QNetworkAccessManager *create(QObject *parent) override;
};

#endif // MURALISNETWORK_H

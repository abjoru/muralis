#include "muralisnetwork.h"
#include "useragent.h"

MuralisNetworkAccessManager::MuralisNetworkAccessManager(QObject *parent)
    : QNetworkAccessManager(parent) {}

QNetworkReply *MuralisNetworkAccessManager::createRequest(Operation op,
                                                          const QNetworkRequest &request,
                                                          QIODevice *outgoingData) {
    QNetworkRequest identified(request);
    identified.setHeader(QNetworkRequest::UserAgentHeader, muralisUserAgent());
    return QNetworkAccessManager::createRequest(op, identified, outgoingData);
}

QNetworkAccessManager *MuralisNetworkAccessManagerFactory::create(QObject *parent) {
    return new MuralisNetworkAccessManager(parent);
}

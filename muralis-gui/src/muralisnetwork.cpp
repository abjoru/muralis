#include "muralisnetwork.h"
#include "useragent.h"

QStringList hotlinkProtectedHosts() {
    // Exact hosts, never a suffix test: `ultrawidewallpapers.net.example.com`
    // is somebody else's host and is told nothing.
    return {QStringLiteral("ultrawidewallpapers.net"),
            QStringLiteral("www.ultrawidewallpapers.net")};
}

QString refererFor(const QUrl &url, const QStringList &protectedHosts) {
    if (!protectedHosts.contains(url.host(), Qt::CaseInsensitive))
        return QString();
    QUrl gallery(url);
    gallery.setPath(QStringLiteral("/gallery"));
    gallery.setQuery(QString());
    gallery.setFragment(QString());
    return gallery.toString();
}

MuralisNetworkAccessManager::MuralisNetworkAccessManager(QObject *parent)
    : QNetworkAccessManager(parent), protectedHosts(hotlinkProtectedHosts()) {}

void MuralisNetworkAccessManager::setProtectedHosts(const QStringList &hosts) {
    protectedHosts = hosts;
}

QNetworkReply *MuralisNetworkAccessManager::createRequest(Operation op,
                                                          const QNetworkRequest &request,
                                                          QIODevice *outgoingData) {
    QNetworkRequest identified(request);
    identified.setHeader(QNetworkRequest::UserAgentHeader, muralisUserAgent());
    // A gated host is told which page the request came from — the gallery this
    // GUI is rendering, which is exactly the request the site expects from the
    // page it serves. Every other host is told nothing.
    const QString referer = refererFor(request.url(), protectedHosts);
    if (!referer.isEmpty())
        identified.setRawHeader("Referer", referer.toUtf8());
    return QNetworkAccessManager::createRequest(op, identified, outgoingData);
}

QNetworkAccessManager *MuralisNetworkAccessManagerFactory::create(QObject *parent) {
    return new MuralisNetworkAccessManager(parent);
}

#ifndef MURALISNETWORK_H
#define MURALISNETWORK_H

#include <QNetworkAccessManager>
#include <QQmlNetworkAccessManagerFactory>
#include <QStringList>
#include <QUrl>

// The hosts whose full-resolution images are gated on the page a request says
// it came from. One site: ultrawidewallpapers.net, which the Ultrawide Source
// browses — an un-refered request for a master is refused at its origin, and
// only its CDN having the master cached ever made that look intermittent.
// Mirrors `gallery_referer` in muralis-source-ultrawide: the GUI's drawer and
// the CLI's download fetch the same protected paths, so they send the same
// header.
QStringList hotlinkProtectedHosts();

// The page a request to `url` claims to come from, or an empty string for a
// host that gates nothing. Always the gallery on the *host being fetched from*:
// the site answers on two hosts and redirects one to the other, and a referer
// naming the host that was not asked describes a page the request did not come
// from.
QString refererFor(const QUrl &url, const QStringList &protectedHosts);

// Every request the QML engine issues — a drawer's full-resolution Image as
// much as anything else — goes out identified, and a request to a gated host
// goes out saying which page it came from. Stamping both here rather than at
// each call site means a future Image can reintroduce neither an anonymous
// request nor one the site will refuse.
class MuralisNetworkAccessManager : public QNetworkAccessManager {
    Q_OBJECT
public:
    explicit MuralisNetworkAccessManager(QObject *parent = nullptr);

    // Defaults to hotlinkProtectedHosts(). Settable so what goes on the wire
    // is asserted against a local test server rather than the live site.
    void setProtectedHosts(const QStringList &hosts);

protected:
    QNetworkReply *createRequest(Operation op, const QNetworkRequest &request,
                                 QIODevice *outgoingData = nullptr) override;

private:
    QStringList protectedHosts;
};

class MuralisNetworkAccessManagerFactory : public QQmlNetworkAccessManagerFactory {
public:
    QNetworkAccessManager *create(QObject *parent) override;
};

#endif // MURALISNETWORK_H

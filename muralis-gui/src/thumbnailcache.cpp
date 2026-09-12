#include "thumbnailcache.h"
#include "muralisnetwork.h"

#include <QCryptographicHash>
#include <QDir>
#include <QFileInfo>
#include <QNetworkReply>
#include <QSaveFile>
#include <QUrl>

ThumbnailCache::ThumbnailCache(const QString &cacheDir, QObject *parent)
    : QObject(parent), dir(cacheDir), nam(new MuralisNetworkAccessManager(this)) {
    QDir().mkpath(dir);
}

// The whole URL is the key, digested: two hosts' `/preview/1.jpg` are two
// images, and a filename alone would collapse them into one entry. No `_thumb`
// suffix — that names the daemon's thumbnail for a kept Wallpaper, which lives
// in the same directory.
QString ThumbnailCache::pathFor(const QString &remoteUrl) const {
    const QByteArray digest =
        QCryptographicHash::hash(remoteUrl.toUtf8(), QCryptographicHash::Sha256).toHex();
    const QString suffix = QFileInfo(QUrl(remoteUrl).path()).suffix();
    QString name = QString::fromLatin1(digest);
    if (!suffix.isEmpty())
        name += "." + suffix;
    return dir + "/" + name;
}

QString ThumbnailCache::request(const QString &remoteUrl) {
    if (remoteUrl.isEmpty())
        return QString();

    const QString local = pathFor(remoteUrl);
    if (QFileInfo::exists(local))
        return QUrl::fromLocalFile(local).toString();

    // One fetch per distinct thumbnail: a grid that renders the same URL twice,
    // or re-renders it, joins the request already in flight.
    if (inFlight.contains(remoteUrl))
        return QString();
    inFlight.insert(remoteUrl, true);

    auto *reply = nam->get(QNetworkRequest(QUrl(remoteUrl)));
    connect(reply, &QNetworkReply::finished, this, [this, reply, remoteUrl, local]() {
        reply->deleteLater();
        inFlight.remove(remoteUrl);

        if (reply->error() != QNetworkReply::NoError) {
            emit failed(remoteUrl, reply->errorString());
            return;
        }
        // Written whole or not at all — a torn file would be served forever as
        // a broken image, and nothing would refetch it.
        QSaveFile file(local);
        if (!file.open(QIODevice::WriteOnly) || file.write(reply->readAll()) < 0
            || !file.commit()) {
            emit failed(remoteUrl, file.errorString());
            return;
        }
        emit ready(remoteUrl, QUrl::fromLocalFile(local).toString());
    });
    return QString();
}

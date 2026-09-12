#ifndef THUMBNAILCACHE_H
#define THUMBNAILCACHE_H

#include <QHash>
#include <QObject>
#include <QString>

class MuralisNetworkAccessManager;

// Preview thumbnails on disk, under the cache directory `muralis cache stats`
// already accounts for and `muralis cache prune` already trims.
class ThumbnailCache : public QObject {
    Q_OBJECT
public:
    explicit ThumbnailCache(const QString &cacheDir, QObject *parent = nullptr);

    // The local file URL for a remote thumbnail, or an empty string when it is
    // not held yet — in which case one fetch is started and `ready` or `failed`
    // follows. A URL already in flight starts no second fetch.
    Q_INVOKABLE QString request(const QString &remoteUrl);

signals:
    void ready(const QString &remoteUrl, const QString &localUrl);
    void failed(const QString &remoteUrl, const QString &reason);

private:
    QString pathFor(const QString &remoteUrl) const;

    QString dir;
    MuralisNetworkAccessManager *nam;
    QHash<QString, bool> inFlight;
};

#endif // THUMBNAILCACHE_H

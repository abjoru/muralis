#include <QDir>
#include <QNetworkReply>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTest>

#include "../src/muralisnetwork.h"
#include "../src/thumbnailcache.h"
#include "../src/useragent.h"
#include "testserver.h"

class TestThumbnailCache : public QObject {
    Q_OBJECT

private slots:
    // A thumbnail the GUI has never seen is fetched once, identified, and left
    // on disk where the cache commands can see it.
    void a_thumbnail_is_fetched_once_and_written_to_the_cache_directory() {
        QTemporaryDir cacheDir;
        TestServer server;
        ThumbnailCache cache(cacheDir.path());

        QSignalSpy arrived(&cache, &ThumbnailCache::ready);
        QVERIFY(cache.request(server.url("/thumb.jpg")).isEmpty());
        QVERIFY(arrived.wait(5000));

        QCOMPARE(server.requestCount(), 1);
        QCOMPARE(server.userAgents().first(), muralisUserAgent());

        const QStringList held = QDir(cacheDir.path()).entryList(QDir::Files);
        QCOMPARE(held.size(), 1);
        QCOMPARE(arrived.first().at(1).toString(),
                 QUrl::fromLocalFile(cacheDir.path() + "/" + held.first()).toString());
    }

    // Rendering a grid a second time costs nothing: the entry on disk answers,
    // and it keeps answering after the GUI has been restarted — which a
    // per-process in-memory image cache cannot do.
    void a_held_thumbnail_is_served_without_a_request_and_survives_a_restart() {
        QTemporaryDir cacheDir;
        TestServer server;
        const QString url = server.url("/thumb.jpg");

        ThumbnailCache first(cacheDir.path());
        QSignalSpy arrived(&first, &ThumbnailCache::ready);
        first.request(url);
        QVERIFY(arrived.wait(5000));
        const QString local = arrived.first().at(1).toString();

        QCOMPARE(first.request(url), local);
        QCOMPARE(server.requestCount(), 1);

        // A new cache over the same directory is the next launch of the GUI.
        ThumbnailCache relaunched(cacheDir.path());
        QCOMPARE(relaunched.request(url), local);
        QCOMPARE(server.requestCount(), 1);
    }

    // Two images whose URLs end in the same filename are two images.
    void urls_sharing_a_filename_are_held_separately() {
        QTemporaryDir cacheDir;
        TestServer server;
        ThumbnailCache cache(cacheDir.path());
        QSignalSpy arrived(&cache, &ThumbnailCache::ready);

        cache.request(server.url("/space/pic.jpg"));
        cache.request(server.url("/nature/pic.jpg"));
        QTRY_COMPARE(arrived.count(), 2);

        QCOMPARE(QDir(cacheDir.path()).entryList(QDir::Files).size(), 2);
        QVERIFY(arrived.at(0).at(1).toString() != arrived.at(1).at(1).toString());
    }

    // A refused thumbnail is a failure the card can render as one, and nothing
    // is left on disk to be served as a broken image forever after.
    void a_thumbnail_the_host_refuses_fails_and_is_not_held() {
        QTemporaryDir cacheDir;
        TestServer server;
        ThumbnailCache cache(cacheDir.path());

        QSignalSpy broke(&cache, &ThumbnailCache::failed);
        cache.request(server.url("/missing.jpg"));
        QVERIFY(broke.wait(5000));

        QVERIFY(QDir(cacheDir.path()).entryList(QDir::Files).isEmpty());

        // And it is retried on the next render rather than stuck in flight.
        cache.request(server.url("/missing.jpg"));
        QVERIFY(broke.wait(5000));
        QCOMPARE(server.requestCount(), 2);
    }

    // The drawer's full-resolution image goes out identified like every other
    // request, but it is large and looked at once: fetching one leaves the
    // thumbnail cache exactly as it was.
    void a_full_resolution_fetch_is_identified_and_not_held() {
        QTemporaryDir cacheDir;
        TestServer server;
        ThumbnailCache cache(cacheDir.path());
        QSignalSpy arrived(&cache, &ThumbnailCache::ready);
        cache.request(server.url("/thumb.jpg"));
        QVERIFY(arrived.wait(5000));
        QCOMPARE(QDir(cacheDir.path()).entryList(QDir::Files).size(), 1);

        MuralisNetworkAccessManager engineNam;
        auto *reply = engineNam.get(QNetworkRequest(QUrl(server.url("/full/master.jpg"))));
        QSignalSpy done(reply, &QNetworkReply::finished);
        QVERIFY(done.wait(5000));

        QCOMPARE(server.userAgents().last(), muralisUserAgent());
        QCOMPARE(QDir(cacheDir.path()).entryList(QDir::Files).size(), 1);
    }
};

QTEST_MAIN(TestThumbnailCache)
#include "tst_thumbnailcache.moc"

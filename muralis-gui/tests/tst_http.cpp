#include <QNetworkReply>
#include <QSignalSpy>
#include <QTest>

#include "../src/muralisnetwork.h"
#include "../src/useragent.h"
#include "testserver.h"

class TestHttp : public QObject {
    Q_OBJECT

private slots:
    // The GUI's network layer identifies muralis on every request it issues,
    // which is what a host that refuses anonymous requests is looking for.
    void every_request_the_engine_issues_identifies_muralis() {
        TestServer server;
        MuralisNetworkAccessManager nam;

        auto *reply = nam.get(QNetworkRequest(QUrl(server.url("/thumb.jpg"))));
        QSignalSpy done(reply, &QNetworkReply::finished);
        QVERIFY(done.wait(5000));

        QCOMPARE(server.userAgents().size(), 1);
        QCOMPARE(server.userAgents().first(), muralisUserAgent());
        QVERIFY(!muralisUserAgent().isEmpty());
    }

    // The drawer's full-resolution image comes off a host that hotlink-protects
    // it: an un-refered request is refused at the origin, and only a CDN cache
    // hit ever hid that. So a request to that host says which page it came
    // from, exactly as the CLI's download does — on the same host it is
    // fetching from, since the site redirects one host to the other and a
    // redirect is a second request.
    void a_request_to_a_gated_host_says_which_page_it_came_from() {
        TestServer server;
        MuralisNetworkAccessManager nam;
        nam.setProtectedHosts({QStringLiteral("127.0.0.1")});

        auto *reply = nam.get(QNetworkRequest(QUrl(server.url("/wallpapers/329/highres/a.jpg"))));
        QSignalSpy done(reply, &QNetworkReply::finished);
        QVERIFY(done.wait(5000));

        QCOMPARE(server.referers().size(), 1);
        QCOMPARE(server.referers().first(),
                 QUrl(server.url("/gallery")).toString());
    }

    // And no other host is told where muralis has been. The header exists for
    // one site's gate; it is not identification, and it is not ours to send
    // everywhere.
    void no_other_host_is_told_where_the_request_came_from() {
        TestServer server;
        MuralisNetworkAccessManager nam;

        auto *reply = nam.get(QNetworkRequest(QUrl(server.url("/thumb.jpg"))));
        QSignalSpy done(reply, &QNetworkReply::finished);
        QVERIFY(done.wait(5000));

        QCOMPARE(server.referers().size(), 1);
        QVERIFY2(server.referers().first().isEmpty(),
                 qPrintable(server.referers().first()));
    }

    // The default is the site the Ultrawide Source browses, on whichever of
    // its two hosts a Preview names.
    void the_gated_host_is_the_site_the_ultrawide_source_browses() {
        QCOMPARE(refererFor(QUrl("https://ultrawidewallpapers.net/wallpapers/329/highres/a.jpg"),
                            hotlinkProtectedHosts()),
                 QStringLiteral("https://ultrawidewallpapers.net/gallery"));
        QCOMPARE(refererFor(QUrl("https://www.ultrawidewallpapers.net/wallpapers/1/highres/a.jpg"),
                            hotlinkProtectedHosts()),
                 QStringLiteral("https://www.ultrawidewallpapers.net/gallery"));
        QVERIFY(refererFor(QUrl("https://w.wallhaven.cc/full/x/a.jpg"), hotlinkProtectedHosts())
                    .isEmpty());
        QVERIFY(
            refererFor(QUrl("https://ultrawidewallpapers.net.example.com/wallpapers/a.jpg"),
                       hotlinkProtectedHosts())
                .isEmpty());
    }
};

QTEST_MAIN(TestHttp)
#include "tst_http.moc"

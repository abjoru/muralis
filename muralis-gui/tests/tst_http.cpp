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
};

QTEST_MAIN(TestHttp)
#include "tst_http.moc"

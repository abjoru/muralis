#ifndef TESTSERVER_H
#define TESTSERVER_H

#include <QByteArray>
#include <QStringList>
#include <QTcpServer>
#include <QTcpSocket>

// A local HTTP server, so what muralis actually puts on the wire can be
// asserted without reaching a live site.
class TestServer : public QObject {
    Q_OBJECT
public:
    explicit TestServer(QObject *parent = nullptr) : QObject(parent) {
        server.listen(QHostAddress::LocalHost, 0);
        connect(&server, &QTcpServer::newConnection, this, &TestServer::serve);
    }

    QString url(const QString &path) const {
        return QStringLiteral("http://127.0.0.1:%1%2").arg(server.serverPort()).arg(path);
    }

    int requestCount() const { return paths.size(); }
    QStringList requestedPaths() const { return paths; }
    QStringList userAgents() const { return agents; }
    // What each request said it came from — empty where it said nothing. The
    // site behind the Ultrawide Source gates its full-resolution images on it.
    QStringList referers() const { return refs; }
    void forget() {
        paths.clear();
        agents.clear();
        refs.clear();
    }

    // The body every 200 carries: a real 1x1 PNG, so a cached file is a
    // decodable image and not just bytes.
    static QByteArray pngBody() {
        return QByteArray::fromBase64("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4"
                                      "2mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==");
    }

private slots:
    void serve() {
        auto *socket = server.nextPendingConnection();
        connect(socket, &QTcpSocket::readyRead, this, [this, socket]() {
            buffer += socket->readAll();
            if (!buffer.contains("\r\n\r\n"))
                return;
            const QByteArray head = buffer.left(buffer.indexOf("\r\n\r\n"));
            buffer.clear();

            const QList<QByteArray> lines = head.split('\n');
            const QString path = QString::fromUtf8(lines.first().split(' ').value(1)).trimmed();
            QString agent;
            QString referer;
            for (const QByteArray &line : lines) {
                if (line.toLower().startsWith("user-agent:"))
                    agent = QString::fromUtf8(line.mid(line.indexOf(':') + 1)).trimmed();
                if (line.toLower().startsWith("referer:"))
                    referer = QString::fromUtf8(line.mid(line.indexOf(':') + 1)).trimmed();
            }
            paths << path;
            agents << agent;
            refs << referer;

            QByteArray response;
            if (path.contains(QStringLiteral("missing"))) {
                response = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            } else {
                const QByteArray body = pngBody();
                response = "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: "
                    + QByteArray::number(body.size()) + "\r\nConnection: close\r\n\r\n" + body;
            }
            socket->write(response);
            socket->disconnectFromHost();
        });
        connect(socket, &QTcpSocket::disconnected, socket, &QTcpSocket::deleteLater);
    }

private:
    QTcpServer server;
    QByteArray buffer;
    QStringList paths;
    QStringList agents;
    QStringList refs;
};

#endif // TESTSERVER_H

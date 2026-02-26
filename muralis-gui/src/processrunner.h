#ifndef PROCESSRUNNER_H
#define PROCESSRUNNER_H

#include <QObject>
#include <QProcess>

class ProcessRunner : public QObject {
    Q_OBJECT

public:
    explicit ProcessRunner(QObject *parent = nullptr);

    Q_INVOKABLE void run(const QString &requestId, const QStringList &args);

signals:
    void finished(const QString &requestId, const QString &stdoutData, int exitCode);
};

#endif // PROCESSRUNNER_H

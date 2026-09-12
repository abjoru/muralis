#include "processrunner.h"

ProcessRunner::ProcessRunner(QObject *parent) : QObject(parent) {}

void ProcessRunner::run(const QString &requestId, const QStringList &args) {
    auto *proc = new QProcess(this);
    connect(proc, QOverload<int, QProcess::ExitStatus>::of(&QProcess::finished),
            this, [this, proc, requestId](int exitCode, QProcess::ExitStatus) {
                QString output = QString::fromUtf8(proc->readAllStandardOutput());
                QString errors = QString::fromUtf8(proc->readAllStandardError());
                emit finished(requestId, output, errors, exitCode);
                proc->deleteLater();
            });
    connect(proc, &QProcess::errorOccurred, this,
            [this, proc, requestId](QProcess::ProcessError) {
                // A CLI that never started reports through the same channel as
                // one that failed, so the UI has something to show either way.
                emit finished(requestId, QString(), proc->errorString(), -1);
                proc->deleteLater();
            });
    proc->start("muralis", args);
}

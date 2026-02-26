#include "processrunner.h"

ProcessRunner::ProcessRunner(QObject *parent) : QObject(parent) {}

void ProcessRunner::run(const QString &requestId, const QStringList &args) {
    auto *proc = new QProcess(this);
    connect(proc, QOverload<int, QProcess::ExitStatus>::of(&QProcess::finished),
            this, [this, proc, requestId](int exitCode, QProcess::ExitStatus) {
                QString output = QString::fromUtf8(proc->readAllStandardOutput());
                emit finished(requestId, output, exitCode);
                proc->deleteLater();
            });
    proc->start("muralis", args);
}

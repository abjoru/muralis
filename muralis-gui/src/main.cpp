#include <QDebug>
#include <QGuiApplication>
#include <cstdio>

void messageHandler(QtMsgType type, const QMessageLogContext &ctx, const QString &msg) {
    Q_UNUSED(ctx);
    const char *prefix = "";
    switch (type) {
    case QtDebugMsg: prefix = "debug"; break;
    case QtWarningMsg: prefix = "warn"; break;
    case QtCriticalMsg: prefix = "crit"; break;
    case QtFatalMsg: prefix = "fatal"; break;
    case QtInfoMsg: prefix = "info"; break;
    }
    fprintf(stderr, "[%s] %s\n", prefix, qPrintable(msg));
    fflush(stderr);
}
#include <QQmlApplicationEngine>
#include <QQuickStyle>
#include <QQmlContext>
#include <QDir>
#include <QStandardPaths>
#include <QJsonDocument>
#include <QJsonObject>
#include <QFile>
#include "processrunner.h"

static QJsonObject readJsonFile(const QString &path) {
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly)) return {};
    return QJsonDocument::fromJson(file.readAll()).object();
}

int main(int argc, char *argv[]) {
    qInstallMessageHandler(messageHandler);
    qputenv("QML_XHR_ALLOW_FILE_READ", "1");

    // Determine dark mode before creating the app
    QString stateDir = QStandardPaths::writableLocation(QStandardPaths::GenericDataLocation)
                           .replace("/share", "/state");
    auto session = readJsonFile(stateDir + "/DankMaterialShell/session.json");
    bool isDark = !session.value("isLightMode").toBool(false);

    // Set Material theme via env before QGuiApplication
    qputenv("QT_QUICK_CONTROLS_MATERIAL_THEME", isDark ? "Dark" : "Light");

    QGuiApplication app(argc, argv);
    app.setApplicationName("muralis-gui");
    app.setOrganizationName("muralis");

    QQuickStyle::setStyle("Material");

    QQmlApplicationEngine engine;

    auto *runner = new ProcessRunner(&app);
    engine.rootContext()->setContextProperty("CLI", runner);

    // Expose paths for Theme.qml
    QString configDir = QStandardPaths::writableLocation(QStandardPaths::ConfigLocation);
    engine.rootContext()->setContextProperty("ConfigDir", configDir);
    engine.rootContext()->setContextProperty("StateDir", stateDir);

    // Ensure the QML engine finds the embedded module qmldir
    engine.addImportPath(QStringLiteral("qrc:/"));

    fprintf(stderr, "Loading QML...\n");
    fprintf(stderr, "ConfigDir: %s\n", qPrintable(configDir));
    fprintf(stderr, "StateDir: %s\n", qPrintable(stateDir));
    fprintf(stderr, "isDark: %d\n", isDark);

    engine.load(QUrl(QStringLiteral("qrc:/MuralisGui/qml/main.qml")));
    if (engine.rootObjects().isEmpty()) {
        fprintf(stderr, "Failed to load QML\n");
        return -1;
    }
    fprintf(stderr, "QML loaded OK, %d root objects\n", (int)engine.rootObjects().size());

    return app.exec();
}

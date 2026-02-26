pragma Singleton
import QtQuick

QtObject {
    id: root

    // DMS color properties — defaults are Gruvbox Material Dark/Hard
    property color primary: "#a8b665"
    property color primaryContainer: "#555c34"
    property color primaryText: "#141617"
    property color secondary: "#d7a657"
    property color surface: "#141617"
    property color surfaceVariant: "#1d2021"
    property color surfaceTint: "#333e34"
    property color surfaceText: "#ddc7a1"
    property color surfaceVariantText: "#d4be98"
    property color background: "#1d2021"
    property color backgroundText: "#ddc7a1"
    property color surfaceContainer: "#282828"
    property color surfaceContainerHigh: "#3c3836"
    property color surfaceContainerHighest: "#504945"
    property color outline: "#a89984"
    property color error: "#e96962"
    property color warning: "#e68a4e"
    property color info: "#d7a657"

    property bool isDark: true
    property real cornerRadius: 12
    property real popupTransparency: 1
    property string fontFamily: "Inter Variable"
    property string monoFontFamily: "Fira Code"
    property real fontScale: 1

    readonly property real spacingXS: 4
    readonly property real spacingS: 8
    readonly property real spacingM: 12
    readonly property real spacingL: 16
    readonly property real spacingXL: 24

    function withAlpha(c, a) {
        return Qt.rgba(c.r, c.g, c.b, a)
    }

    Component.onCompleted: {
        loadDmsTheme()
    }

    function loadDmsTheme() {
        // ConfigDir and StateDir are set by C++ main.cpp
        var settingsPath = ConfigDir + "/DankMaterialShell/settings.json"
        var sessionPath = StateDir + "/DankMaterialShell/session.json"

        // Load session for light/dark mode
        var sessionData = readJson(sessionPath)
        if (sessionData) {
            isDark = !sessionData.isLightMode
        }

        // Load settings for theme file path + variant selection
        var settings = readJson(settingsPath)
        if (!settings || !settings.customThemeFile) return

        var themeData = readJson(settings.customThemeFile)
        if (!themeData) return

        if (settings.cornerRadius !== undefined) cornerRadius = settings.cornerRadius
        if (settings.popupTransparency !== undefined) popupTransparency = settings.popupTransparency
        if (settings.fontFamily) fontFamily = settings.fontFamily
        if (settings.monoFontFamily) monoFontFamily = settings.monoFontFamily
        if (settings.fontScale !== undefined) fontScale = settings.fontScale

        var mode = isDark ? "dark" : "light"
        var modeColors = themeData[mode]
        if (modeColors) {
            if (modeColors.primary) primary = modeColors.primary
            if (modeColors.primaryContainer) primaryContainer = modeColors.primaryContainer
            if (modeColors.secondary) secondary = modeColors.secondary
            if (modeColors.surfaceText) surfaceText = modeColors.surfaceText
            if (modeColors.surfaceVariantText) surfaceVariantText = modeColors.surfaceVariantText
            if (modeColors.backgroundText) backgroundText = modeColors.backgroundText
            if (modeColors.outline) outline = modeColors.outline
            if (modeColors.error) error = modeColors.error
            if (modeColors.warning) warning = modeColors.warning
            if (modeColors.info) info = modeColors.info
        }

        // Apply variant surface colors
        if (themeData.variants && themeData.variants.options) {
            var variantId = themeData.variants["default"] || ""
            if (settings.registryThemeVariants && themeData.id) {
                var v = settings.registryThemeVariants[themeData.id]
                if (v) variantId = v
            }

            for (var i = 0; i < themeData.variants.options.length; i++) {
                var opt = themeData.variants.options[i]
                if (opt.id === variantId) {
                    var vc = opt[mode]
                    if (vc) {
                        if (vc.primaryText) primaryText = vc.primaryText
                        if (vc.surface) surface = vc.surface
                        if (vc.surfaceVariant) surfaceVariant = vc.surfaceVariant
                        if (vc.surfaceTint) surfaceTint = vc.surfaceTint
                        if (vc.background) background = vc.background
                        if (vc.surfaceContainer) surfaceContainer = vc.surfaceContainer
                        if (vc.surfaceContainerHigh) surfaceContainerHigh = vc.surfaceContainerHigh
                        if (vc.surfaceContainerHighest) surfaceContainerHighest = vc.surfaceContainerHighest
                    }
                    break
                }
            }
        }
    }

    function readJson(path) {
        var xhr = new XMLHttpRequest()
        xhr.open("GET", "file://" + path, false)
        try {
            xhr.send()
            if (xhr.status === 200 || xhr.status === 0) {
                return JSON.parse(xhr.responseText)
            }
        } catch (e) {}
        return null
    }
}

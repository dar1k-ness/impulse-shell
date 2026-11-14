pragma Singleton
pragma ComponentBehavior: Bound

import ri.modules.common.functions
import ri.modules.common
import QtQuick
import Rustshell
import Impulse

/**
 * Renders LaTeX snippets with MicroTeX.
 * For every request:
 *   1. Hash it
 *   2. Check if the hash is already processed
 *   3. If not, render it with MicroTeX and mark as processed
 */
Singleton {
    id: root
    
    readonly property var renderPadding: 4 // This is to prevent cutoff in the rendered images

    property list<string> processedHashes: []
    property var processedExpressions: ({})
    property var renderedImagePaths: ({})
    property string microtexBinaryDir: "/opt/MicroTeX"
    property string microtexBinaryName: "LaTeX"
    property string latexOutputPath: Directories.latexOutput

    signal renderFinished(string hash, string imagePath)

    /**
    * Requests rendering of a LaTeX expression.
    * Returns the [hash, isNew]
    */
    function requestRender(expression) {
        // 1. Hash it and initialize necessary variables
        const hash = Qt.md5(expression)
        const imagePath = `${latexOutputPath}/${hash}.svg`

        // MicroTeX fails on an empty formula: nothing to render or to remember
        if (expression === "") return [hash, false]

        // 2. Check if the hash is already processed; report only an image that was made
        if (processedHashes.includes(hash)) {
            if (renderedImagePaths[hash] !== undefined)
                renderFinished(hash, imagePath)
            return [hash, false]
        } else {
            root.processedHashes.push(hash)
            root.processedExpressions[hash] = expression
        }

        // 3. If not, render it with MicroTeX and mark as processed
        microtexComponent.createObject(root, {
            hash: hash,
            binaryDir: root.microtexBinaryDir,
            binaryName: root.microtexBinaryName,
            expression: expression,
            output: imagePath,
            textSize: `${Appearance.font.pixelSize.normal}`,
            padding: `${root.renderPadding}`,
            foreground: `${Appearance.colors.colOnLayer1}`,
            running: true
        })
        return [hash, true]
    }

    Component {
        id: microtexComponent
        MicroTexRender {
            id: microtexProcess
            property string hash
            onExited: (exitCode, exitStatus) => {
                if (exitCode === 0) {
                    root.renderedImagePaths[microtexProcess.hash] = microtexProcess.output
                    root.renderFinished(microtexProcess.hash, microtexProcess.output)
                } else {
                    // Not processed: the next request for this formula renders it again.
                    console.warn(`[LatexRenderer] Rendering failed with exit code ${exitCode}: ${microtexProcess.expression}`)
                    root.processedHashes = root.processedHashes.filter(hash => hash !== microtexProcess.hash)
                    delete root.processedExpressions[microtexProcess.hash]
                }
                microtexProcess.destroy()
            }
        }
    }
}

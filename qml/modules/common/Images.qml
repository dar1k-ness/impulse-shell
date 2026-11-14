pragma Singleton

import Rustshell

Singleton {
    // Formats
    readonly property list<string> validImageTypes: ["jpeg", "png", "webp", "tiff", "svg"]
    readonly property list<string> validImageExtensions: ["jpg", "jpeg", "png", "webp", "tif", "tiff", "svg"]

    function isValidImageByName(name: string): bool {
        return validImageExtensions.some(t => name.endsWith(`.${t}`));
    }
    readonly property list<string> videoExtensions: ["mp4", "webm", "mkv", "avi", "mov"]
    function isVideoByName(name: string): bool {
        return videoExtensions.some(t => name.endsWith(`.${t}`));
    }

    // The wallpaper as a file an Image can load: a video's still frame, else the wallpaper itself
    readonly property string wallpaperStillPath: isVideoByName(Config.options.background.wallpaperPath) ? Config.options.background.thumbnailPath : Config.options.background.wallpaperPath

    // Thumbnails
    // https://specifications.freedesktop.org/thumbnail-spec/latest/directory.html
    readonly property var thumbnailSizes: ({
        "normal": 128,
        "large": 256,
        "x-large": 512,
        "xx-large": 1024
    })
    function thumbnailSizeNameForDimensions(width: int, height: int): string {
        const sizeNames = Object.keys(thumbnailSizes);
        for(let i = 0; i < sizeNames.length; i++) {
            const sizeName = sizeNames[i];
            const maxSize = thumbnailSizes[sizeName];
            if (width <= maxSize && height <= maxSize) return sizeName;
        }
        return "xx-large";
    }
}

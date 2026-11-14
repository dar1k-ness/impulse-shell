//! Canvas' Context2D against Qt itself (`qml6`, offscreen): every case is a paint function run
//! on a 48x32 Canvas by Qt and by us; the pixels `getImageData` returns and the value the
//! function returns are compared (the pixels as they are on the premultiplied image). Plus
//! the two regressions of the rendering audit: the acrylic edge line (createLinearGradient +
//! arcTo) and non-finite arguments (a dashed stroke after `scale(NaN, 1)` used to hang).

use qsg::raster::Canvas;
use shell::scene::{self, Target};

const CW: usize = 48;
const CH: usize = 32;

/// (name, JS body of `function(ctx)`); the returned value is compared as a string.
fn cases() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = Vec::new();
    let mut add = |n: &str, b: &str| v.push((n.to_string(), b.to_string()));
    add(
        "fillRect",
        "ctx.fillStyle = '#3070c0'; ctx.fillRect(3.5, 2.25, 20, 11.5);",
    );
    add(
        "fillRect alpha",
        "ctx.fillStyle = 'rgba(255, 0, 0, 0.5)'; ctx.globalAlpha = 0.6; ctx.fillRect(2, 2, 30, 20); \
         ctx.fillStyle = Qt.rgba(0, 0, 1, 0.5); ctx.fillRect(10.3, 8.7, 30, 20);",
    );
    add(
        "strokeRect",
        "ctx.strokeStyle = '#208040'; ctx.lineWidth = 3; ctx.strokeRect(5, 4, 30, 20);",
    );
    add(
        "clearRect",
        "ctx.fillStyle = 'orange'; ctx.fillRect(0, 0, 48, 32); ctx.clearRect(4.5, 3.25, 20, 10.5);",
    );
    add(
        "thin lines",
        "ctx.beginPath(); ctx.moveTo(2, 3); ctx.lineTo(44, 25); ctx.moveTo(4.5, 28.5); \
         ctx.lineTo(40.5, 28.5); ctx.moveTo(10.5, 2); ctx.lineTo(10.5, 30); ctx.stroke();",
    );
    add(
        "half-width line",
        "ctx.lineWidth = 0.5; ctx.strokeStyle = '#ff0000'; ctx.beginPath(); ctx.moveTo(2, 3); \
         ctx.lineTo(44, 25); ctx.stroke();",
    );
    add(
        "round joins and caps",
        "ctx.lineWidth = 5; ctx.lineJoin = 'round'; ctx.lineCap = 'round'; ctx.strokeStyle = '#8040c0'; \
         ctx.beginPath(); ctx.moveTo(6, 6); ctx.lineTo(24, 26); ctx.lineTo(42, 8); ctx.stroke();",
    );
    add(
        "miter join",
        "ctx.lineWidth = 4; ctx.lineJoin = 'miter'; ctx.miterLimit = 3; ctx.beginPath(); \
         ctx.moveTo(4, 28); ctx.lineTo(14, 4); ctx.lineTo(24, 28); ctx.lineTo(44, 20); ctx.stroke();",
    );
    add(
        "bevel join square cap",
        "ctx.lineWidth = 4; ctx.lineJoin = 'bevel'; ctx.lineCap = 'square'; ctx.beginPath(); \
         ctx.moveTo(6, 26); ctx.lineTo(20, 6); ctx.lineTo(40, 24); ctx.stroke();",
    );
    add(
        "dashed wide line",
        "ctx.lineWidth = 2; ctx.setLineDash([4, 2]); ctx.lineDashOffset = 1; ctx.beginPath(); \
         ctx.moveTo(2, 6); ctx.lineTo(46, 6); ctx.lineTo(46, 28); ctx.stroke(); \
         return ctx.getLineDash().join() + ' ' + ctx.lineDashOffset;",
    );
    add(
        "dashed thin line",
        "ctx.setLineDash([3, 3]); ctx.beginPath(); ctx.moveTo(2.5, 6.5); ctx.lineTo(45.5, 6.5); \
         ctx.moveTo(3, 12); ctx.lineTo(40, 30); ctx.stroke();",
    );
    add(
        "bezier fill",
        "ctx.fillStyle = '#c03030'; ctx.beginPath(); ctx.moveTo(4, 28); \
         ctx.bezierCurveTo(4, 2, 44, 2, 44, 28); ctx.fill();",
    );
    add(
        "quadratic stroke",
        "ctx.lineWidth = 2; ctx.beginPath(); ctx.moveTo(4, 28); \
         ctx.quadraticCurveTo(24, -10, 44, 28); ctx.stroke();",
    );
    add(
        "arc fill",
        "ctx.fillStyle = '#2060a0'; ctx.beginPath(); ctx.arc(24, 16, 12.5, 0, 2 * Math.PI, false); ctx.fill();",
    );
    add(
        "arc stroke",
        "ctx.lineWidth = 3; ctx.beginPath(); ctx.arc(24, 16, 11, 0.4, 4.2, true); ctx.stroke(); \
         ctx.beginPath(); ctx.arc(24, 16, 5, 1, 3, false); ctx.stroke();",
    );
    add(
        "arcTo",
        "ctx.lineWidth = 2; ctx.beginPath(); ctx.moveTo(6, 4.5); ctx.lineTo(36, 4.5); \
         ctx.arcTo(46, 4.5, 46, 14.5, 10); ctx.lineTo(46, 28); ctx.stroke();",
    );
    add(
        "acrylic edge",
        "var w = 48, r = 6; var grad = ctx.createLinearGradient(0, 0, w, 0); \
         grad.addColorStop(0, Qt.rgba(1, 1, 1, 0)); grad.addColorStop(0.2, Qt.rgba(1, 1, 1, 0.6)); \
         grad.addColorStop(0.8, Qt.rgba(1, 1, 1, 0.6)); grad.addColorStop(1, Qt.rgba(1, 1, 1, 0)); \
         ctx.strokeStyle = grad; ctx.lineWidth = 1; ctx.beginPath(); ctx.moveTo(r, 0.5); \
         ctx.lineTo(w - r, 0.5); ctx.arcTo(w, 0.5, w, r + 0.5, r); ctx.moveTo(w - r, 0.5); \
         ctx.arcTo(0, 0.5, 0, r + 0.5, r); ctx.stroke();",
    );
    add(
        "rect fill and stroke",
        "ctx.fillStyle = '#e0c020'; ctx.lineWidth = 2; ctx.beginPath(); ctx.rect(6.5, 5.5, 30, 18); \
         ctx.fill(); ctx.stroke();",
    );
    add(
        "roundedRect",
        "ctx.fillStyle = '#40a080'; ctx.beginPath(); ctx.roundedRect(4, 3, 38, 24, 8, 5); ctx.fill(); \
         ctx.stroke();",
    );
    add(
        "ellipse",
        "ctx.lineWidth = 2.5; ctx.beginPath(); ctx.ellipse(5, 4, 36, 22); ctx.stroke();",
    );
    add(
        "closePath then line",
        "ctx.lineWidth = 3; ctx.beginPath(); ctx.moveTo(6, 6); ctx.lineTo(30, 6); ctx.lineTo(18, 26); \
         ctx.closePath(); ctx.lineTo(44, 28); ctx.stroke();",
    );
    add(
        "odd-even fill",
        "ctx.fillRule = Qt.OddEvenFill; ctx.fillStyle = '#804020'; ctx.beginPath(); \
         ctx.rect(2, 2, 40, 28); ctx.rect(10, 8, 20, 14); ctx.fill(); return '' + ctx.fillRule;",
    );
    add(
        "winding fill",
        "ctx.fillStyle = '#804020'; ctx.beginPath(); ctx.rect(2, 2, 40, 28); ctx.rect(10, 8, 20, 14); \
         ctx.fill(); return '' + ctx.fillRule;",
    );
    add(
        "path string",
        "ctx.path = 'M 4 4 L 44 4 L 24 28 Z'; ctx.fillStyle = '#306090'; ctx.fill();",
    );
    add(
        "text path",
        "ctx.font = '20px sans-serif'; ctx.beginPath(); ctx.text('Qt', 4, 24); ctx.fill();",
    );
    add(
        "fillText",
        "ctx.font = 'bold 14px sans-serif'; ctx.fillStyle = '#103050'; ctx.fillText('Ab7', 3, 20); \
         return ctx.font;",
    );
    add(
        "fillText aligned",
        "ctx.font = '12px sans-serif'; ctx.textAlign = 'center'; ctx.textBaseline = 'top'; \
         ctx.fillText('mid', 24, 2); ctx.textAlign = 'right'; ctx.textBaseline = 'bottom'; \
         ctx.fillText('end', 46, 31); return ctx.textAlign + ' ' + ctx.textBaseline;",
    );
    add(
        "fillText baselines",
        "ctx.font = '10px sans-serif'; ctx.textBaseline = 'middle'; ctx.fillText('m', 2, 16); \
         ctx.textBaseline = 'hanging'; ctx.fillText('h', 14, 16); ctx.textBaseline = 'alphabetic'; \
         ctx.textAlign = 'end'; ctx.fillText('e', 40, 16); ctx.textAlign = 'left'; ctx.fillText('l', 40, 28);",
    );
    add(
        "strokeText",
        "ctx.font = '22px sans-serif'; ctx.strokeStyle = '#a02060'; ctx.strokeText('Q', 12, 24);",
    );
    add(
        "measureText",
        "var s = ''; ctx.font = '10px sans-serif'; s += ctx.measureText('Hello').width; \
         ctx.font = '17px sans-serif'; s += ' ' + ctx.measureText('Hello, World').width; \
         ctx.font = 'bold 12px monospace'; s += ' ' + ctx.measureText('iiii').width; \
         s += ' ' + ctx.measureText('').width; return s;",
    );
    add(
        "linear gradient",
        "var g = ctx.createLinearGradient(4, 0, 44, 20); g.addColorStop(0, 'red'); \
         g.addColorStop(1, 'blue'); ctx.fillStyle = g; ctx.fillRect(2, 2, 44, 28);",
    );
    add(
        "linear gradient stops",
        "var g = ctx.createLinearGradient(0, 0, 48, 0); g.addColorStop(0.1, '#ff0000'); \
         g.addColorStop(0.5, Qt.rgba(0, 1, 0, 0.3)); g.addColorStop(0.5, '#0000ff'); \
         g.addColorStop(0.9, '#ffff00'); ctx.fillStyle = g; ctx.globalAlpha = 0.8; \
         ctx.fillRect(0, 0, 48, 32);",
    );
    add(
        "gradient stroke",
        "var g = ctx.createLinearGradient(0, 0, 0, 32); g.addColorStop(0, '#00ffff'); \
         g.addColorStop(1, '#ff00ff'); ctx.strokeStyle = g; ctx.lineWidth = 6; ctx.beginPath(); \
         ctx.moveTo(6, 4); ctx.lineTo(40, 28); ctx.stroke();",
    );
    add(
        "radial gradient",
        "var g = ctx.createRadialGradient(20, 14, 2, 24, 16, 15); g.addColorStop(0, 'white'); \
         g.addColorStop(0.6, '#2040ff'); g.addColorStop(1, 'black'); ctx.fillStyle = g; \
         ctx.fillRect(0, 0, 48, 32);",
    );
    add(
        "radial gradient simple",
        "var g = ctx.createRadialGradient(24, 16, 0, 24, 16, 14); g.addColorStop(0, '#ff8000'); \
         g.addColorStop(1, Qt.rgba(0, 0, 1, 0)); ctx.fillStyle = g; ctx.beginPath(); \
         ctx.arc(24, 16, 15, 0, 2 * Math.PI, false); ctx.fill();",
    );
    add(
        "conical gradient",
        "var g = ctx.createConicalGradient(24, 16, 0.7); g.addColorStop(0, 'red'); \
         g.addColorStop(0.5, 'lime'); g.addColorStop(1, 'blue'); ctx.fillStyle = g; \
         ctx.fillRect(0, 0, 48, 32);",
    );
    add(
        "gradient under transform",
        "ctx.translate(24, 16); ctx.rotate(0.5); ctx.scale(1.2, 0.8); \
         var g = ctx.createLinearGradient(-15, 0, 15, 0); g.addColorStop(0, '#102030'); \
         g.addColorStop(1, '#f0e0d0'); ctx.fillStyle = g; ctx.fillRect(-15, -10, 30, 20);",
    );
    add(
        "colour pattern",
        "ctx.fillStyle = ctx.createPattern('#c02020', 5); ctx.fillRect(0, 0, 24, 32); \
         ctx.fillStyle = ctx.createPattern(Qt.rgba(0, 0, 1, 1), 11); ctx.fillRect(24.5, 0, 20, 32);",
    );
    add(
        "more colour patterns",
        "var x = 0; [2, 4, 8, 9, 10, 12, 13, 14].forEach(function(s) { \
         ctx.fillStyle = ctx.createPattern('#204080', s); ctx.fillRect(x, 1, 6, 30); x += 6; });",
    );
    add(
        "image pattern",
        "var im = ctx.createImageData(4, 4); for (var i = 0; i < 16; i++) { im.data[4 * i] = 60 * (i % 4); \
         im.data[4 * i + 1] = 255 - 16 * i; im.data[4 * i + 2] = 40; im.data[4 * i + 3] = 255; } \
         ctx.fillStyle = ctx.createPattern(im, 'repeat'); ctx.fillRect(3, 2, 40, 27);",
    );
    add(
        "clip",
        "ctx.beginPath(); ctx.rect(8.5, 6.5, 30, 18); ctx.clip(); ctx.fillStyle = '#d04040'; \
         ctx.beginPath(); ctx.arc(24, 16, 15, 0, 2 * Math.PI, false); ctx.fill();",
    );
    add(
        "clip twice and restore",
        "ctx.save(); ctx.beginPath(); ctx.arc(20, 16, 14, 0, 2 * Math.PI, false); ctx.clip(); \
         ctx.beginPath(); ctx.rect(18, 0, 30, 32); ctx.clip(); ctx.fillStyle = '#4040d0'; \
         ctx.fillRect(0, 0, 48, 32); ctx.restore(); ctx.fillStyle = '#40d040'; ctx.fillRect(0, 26, 48, 4);",
    );
    add(
        "save restore",
        "ctx.fillStyle = '#ff0000'; ctx.save(); ctx.fillStyle = '#0000ff'; ctx.translate(20, 10); \
         ctx.lineWidth = 4; ctx.fillRect(0, 0, 10, 10); ctx.restore(); ctx.fillRect(2, 2, 10, 10); \
         return ctx.fillStyle + ' ' + ctx.lineWidth;",
    );
    add(
        "rotate",
        "ctx.translate(24, 16); ctx.rotate(0.6); ctx.fillStyle = '#207050'; ctx.fillRect(-14, -6, 28, 12); \
         ctx.lineWidth = 2; ctx.strokeRect(-14, -6, 28, 12);",
    );
    add(
        "scale",
        "ctx.scale(2.5, 1.5); ctx.fillStyle = '#705020'; ctx.fillRect(2.2, 3.1, 10, 8); ctx.lineWidth = 2; \
         ctx.beginPath(); ctx.moveTo(1, 1); ctx.lineTo(16, 18); ctx.stroke();",
    );
    add(
        "shear",
        "ctx.shear(0.5, 0.1); ctx.fillStyle = '#502070'; ctx.fillRect(4, 4, 20, 14); ctx.lineWidth = 3; \
         ctx.strokeRect(4, 4, 20, 14);",
    );
    add(
        "transform setTransform resetTransform",
        "ctx.transform(1, 0.2, -0.3, 1, 10, 2); ctx.fillStyle = '#b02020'; ctx.fillRect(0, 0, 12, 12); \
         ctx.setTransform(0.5, 0, 0, 2, 24, 4); ctx.fillStyle = '#2020b0'; ctx.fillRect(0, 0, 30, 10); \
         ctx.resetTransform(); ctx.fillStyle = '#20b020'; ctx.fillRect(40, 26, 6, 4);",
    );
    add(
        "path survives transform",
        "ctx.beginPath(); ctx.moveTo(4, 4); ctx.lineTo(30, 4); ctx.translate(6, 10); ctx.rotate(0.3); \
         ctx.lineTo(20, 10); ctx.scale(2, 1); ctx.lineTo(2, 14); ctx.closePath(); \
         ctx.fillStyle = '#909020'; ctx.fill(); ctx.lineWidth = 1.5; ctx.stroke();",
    );
    add(
        "thin line under scale",
        "ctx.scale(0.5, 0.5); ctx.lineWidth = 1.5; ctx.beginPath(); ctx.moveTo(6, 8); ctx.lineTo(90, 50); \
         ctx.stroke();",
    );
    add(
        "singular transform",
        "ctx.fillStyle = '#ff0000'; ctx.save(); ctx.scale(0, 1); ctx.fillRect(0, 0, 48, 32); \
         ctx.beginPath(); ctx.rect(2, 2, 10, 10); ctx.fill(); ctx.restore(); ctx.fillRect(30, 20, 8, 8);",
    );
    for m in [
        "source-over",
        "source-in",
        "source-out",
        "source-atop",
        "destination-over",
        "destination-in",
        "destination-out",
        "destination-atop",
        "lighter",
        "copy",
        "xor",
        "qt-clear",
        "qt-destination",
        "qt-multiply",
        "qt-screen",
        "qt-overlay",
        "qt-darken",
        "qt-lighten",
        "qt-color-dodge",
        "qt-color-burn",
        "qt-hard-light",
        "qt-soft-light",
        "qt-difference",
        "qt-exclusion",
    ] {
        v.push((
            format!("composite {m}"),
            format!(
                "ctx.fillStyle = 'rgba(40, 120, 220, 0.8)'; ctx.fillRect(4, 3, 28, 18); \
                 ctx.globalCompositeOperation = '{m}'; ctx.fillStyle = 'rgba(230, 150, 30, 0.6)'; \
                 ctx.beginPath(); ctx.arc(30, 18, 11.5, 0, 2 * Math.PI, false); ctx.fill(); \
                 var g = ctx.createLinearGradient(0, 22, 48, 22); g.addColorStop(0, 'rgba(255, 0, 0, 0.9)'); \
                 g.addColorStop(1, 'rgba(0, 255, 0, 0.3)'); ctx.fillStyle = g; ctx.fillRect(2.5, 14.5, 30, 6); \
                 return ctx.globalCompositeOperation;"
            ),
        ));
    }
    let mut add = |n: &str, b: &str| v.push((n.to_string(), b.to_string()));
    add(
        "shadow rect",
        "ctx.shadowColor = '#000000'; ctx.shadowOffsetX = 4; ctx.shadowOffsetY = 3; \
         ctx.fillStyle = '#e04040'; ctx.fillRect(6, 5, 24, 14); return ctx.shadowColor;",
    );
    add(
        "shadow blur",
        "ctx.shadowColor = 'rgba(0, 0, 255, 0.8)'; ctx.shadowBlur = 5; ctx.shadowOffsetX = 2; \
         ctx.fillStyle = '#40e040'; ctx.beginPath(); ctx.arc(22, 15, 8, 0, 2 * Math.PI, false); ctx.fill();",
    );
    add(
        "shadow stroke",
        "ctx.shadowColor = '#804000'; ctx.shadowBlur = 2; ctx.shadowOffsetY = 4; ctx.lineWidth = 3; \
         ctx.beginPath(); ctx.moveTo(6, 6); ctx.lineTo(40, 14); ctx.stroke();",
    );
    add(
        "image data",
        "var im = ctx.createImageData(6, 5); for (var i = 0; i < 30; i++) { im.data[4 * i] = 8 * i; \
         im.data[4 * i + 1] = 255 - 8 * i; im.data[4 * i + 2] = 128; im.data[4 * i + 3] = 100 + 5 * i; } \
         ctx.drawImage(im, 3, 2); ctx.drawImage(im, 12, 2, 30, 20); ctx.drawImage(im, 1, 1, 4, 3, 4, 24, 8, 6); \
         return im.width + 'x' + im.height + ' ' + im.data.length;",
    );
    add(
        "getImageData",
        "ctx.fillStyle = 'rgba(200, 100, 50, 0.5)'; ctx.fillRect(1, 1, 3, 2); \
         var d = ctx.getImageData(0, 0, 3, 2); var s = d.width + 'x' + d.height; \
         for (var i = 0; i < d.data.length; i++) s += ' ' + d.data[i]; return s;",
    );
    add(
        "putImageData",
        "var im = ctx.createImageData(8, 6); for (var i = 0; i < 48; i++) { im.data[4 * i] = 255; \
         im.data[4 * i + 1] = 5 * i; im.data[4 * i + 3] = 255; } ctx.fillStyle = '#0000ff'; \
         ctx.fillRect(0, 0, 48, 32); ctx.putImageData(im, 5, 4); ctx.putImageData(im, 20, 10, 2, 1, 4, 3);",
    );
    add(
        "isPointInPath",
        "ctx.beginPath(); ctx.rect(10, 10, 20, 10); ctx.arc(40, 6, 5, 0, 2 * Math.PI, false); \
         var p = [[15, 15], [10, 10], [30, 20], [31, 15], [9.99, 12], [40, 6], [40, 0.5], [44, 10], \
         [NaN, 1], [20, 10]]; var s = ''; for (var i = 0; i < p.length; i++) \
         s += ctx.isPointInPath(p[i][0], p[i][1]) ? '1' : '0'; ctx.translate(5, 5); \
         s += ctx.isPointInPath(12, 12) ? '1' : '0'; return s;",
    );
    add(
        "defaults",
        "return [ctx.fillStyle, ctx.strokeStyle, ctx.globalAlpha, ctx.globalCompositeOperation, \
         ctx.lineWidth, ctx.lineCap, ctx.lineJoin, ctx.miterLimit, ctx.lineDashOffset, \
         ctx.getLineDash().length, ctx.shadowBlur, ctx.shadowColor, ctx.shadowOffsetX, \
         ctx.shadowOffsetY, ctx.font, ctx.textAlign, ctx.textBaseline, ctx.fillRule, \
         typeof ctx.canvas, typeof ctx.path].join('|');",
    );
    add(
        "setters",
        "var s = []; ctx.fillStyle = 'rgba(10, 20, 30, 0.5)'; s.push(ctx.fillStyle); \
         ctx.fillStyle = 'hsl(120, 50%, 50%)'; s.push(ctx.fillStyle); ctx.fillStyle = 'nonsense'; \
         s.push(ctx.fillStyle); ctx.strokeStyle = Qt.rgba(1, 0, 0, 1); s.push(ctx.strokeStyle); \
         ctx.globalAlpha = 2; s.push(ctx.globalAlpha); ctx.globalAlpha = 0.25; s.push(ctx.globalAlpha); \
         ctx.lineWidth = -1; s.push(ctx.lineWidth); ctx.lineWidth = 0; s.push(ctx.lineWidth); \
         ctx.lineWidth = 2.5; s.push(ctx.lineWidth); ctx.lineCap = 'bogus'; s.push(ctx.lineCap); \
         ctx.lineCap = 'square'; s.push(ctx.lineCap); ctx.lineJoin = 'round'; s.push(ctx.lineJoin); \
         ctx.miterLimit = 0; s.push(ctx.miterLimit); ctx.miterLimit = 4; s.push(ctx.miterLimit); \
         ctx.shadowBlur = -3; s.push(ctx.shadowBlur); ctx.shadowBlur = 3; s.push(ctx.shadowBlur); \
         ctx.shadowColor = 'rgba(1, 2, 3, 0.5)'; s.push(ctx.shadowColor); ctx.shadowOffsetX = Infinity; \
         s.push(ctx.shadowOffsetX); ctx.shadowOffsetY = -2; s.push(ctx.shadowOffsetY); \
         ctx.globalCompositeOperation = 'bogus'; s.push(ctx.globalCompositeOperation); \
         ctx.globalCompositeOperation = 'qt-lighten'; s.push(ctx.globalCompositeOperation); \
         ctx.textAlign = 'bogus'; s.push(ctx.textAlign); ctx.textAlign = 'end'; s.push(ctx.textAlign); \
         ctx.textBaseline = 'hanging'; s.push(ctx.textBaseline); ctx.font = 'italic 13pt serif'; \
         s.push(ctx.font); ctx.font = 'rubbish'; s.push(ctx.font); ctx.lineDashOffset = NaN; \
         s.push(ctx.lineDashOffset); ctx.setLineDash([1, 2, 3]); s.push(ctx.getLineDash().join()); \
         return s.join('|');",
    );
    add(
        "exceptions",
        "var s = []; function t(f) { try { f(); s.push('ok'); } catch (e) { s.push(e.message); } } \
         t(function() { ctx.arc(10, 10, -1, 0, 1, false); }); \
         t(function() { ctx.arcTo(1, 2, 3, 4, -5); }); \
         t(function() { ctx.createLinearGradient(0, NaN, 1, 1); }); \
         t(function() { ctx.createRadialGradient(0, 0, -1, 1, 1, 1); }); \
         t(function() { ctx.createConicalGradient(0, 0, Infinity); }); \
         t(function() { ctx.createLinearGradient(0, 0, 1, 1).addColorStop(2, 'red'); }); \
         t(function() { ctx.drawFocusRing(); }); t(function() { ctx.setCaretSelectionRect(); }); \
         t(function() { ctx.caretBlinkRate(); }); t(function() { ctx.createImageData(0, 0); }); \
         t(function() { ctx.getImageData(0, 0, 0, 0); }); t(function() { ctx.arc(1, 2, NaN, 0, 1); }); \
         return s.join('|');",
    );
    add(
        "non-finite arguments",
        "ctx.fillStyle = '#c04080'; ctx.scale(NaN, 1); ctx.translate(Infinity, 0); ctx.rotate(NaN); \
         ctx.shear(1, -Infinity); ctx.transform(1, 0, 0, NaN, 0, 0); ctx.setTransform(1, 0, 0, 1, NaN, 0); \
         ctx.beginPath(); ctx.moveTo(NaN, 2); ctx.moveTo(4, 4); ctx.lineTo(Infinity, 4); ctx.lineTo(40, 6); \
         ctx.bezierCurveTo(NaN, 0, 0, 0, 0, 0); ctx.quadraticCurveTo(0, NaN, 0, 0); ctx.arc(NaN, 0, 5, 0, 1, false); \
         ctx.arcTo(NaN, 0, 0, 0, 3); ctx.rect(0, 0, NaN, 5); ctx.roundedRect(0, 0, 5, 5, NaN, 1); \
         ctx.ellipse(0, 0, Infinity, 5); ctx.lineTo(30, 28); ctx.closePath(); ctx.fill(); \
         ctx.fillRect(NaN, 0, 5, 5); ctx.strokeRect(0, 0, Infinity, 5); ctx.clearRect(0, NaN, 48, 32); \
         ctx.fillText('x', NaN, 5); ctx.setLineDash([3, 2]); ctx.lineWidth = 2; ctx.stroke();",
    );
    add(
        "reset",
        "ctx.fillStyle = '#ff0000'; ctx.lineWidth = 7; ctx.translate(10, 10); ctx.fillRect(0, 0, 10, 10); \
         ctx.save(); ctx.reset(); ctx.fillRect(1, 1, 4, 4); return ctx.fillStyle + ' ' + ctx.lineWidth;",
    );
    add(
        "wide lines",
        "ctx.strokeStyle = '#3060a0'; ctx.lineWidth = 3.5; ctx.beginPath(); ctx.moveTo(3, 4); \
         ctx.lineTo(44, 9.5); ctx.moveTo(6.25, 12); ctx.lineTo(6.25, 29); ctx.moveTo(12, 28.5); \
         ctx.lineTo(45, 28.5); ctx.moveTo(40, 14); ctx.lineTo(18, 25); ctx.stroke();",
    );
    add(
        "wide lines square cap",
        "ctx.strokeStyle = 'rgba(160, 60, 20, 0.7)'; ctx.lineCap = 'square'; ctx.lineWidth = 4; \
         ctx.beginPath(); ctx.moveTo(8, 6); ctx.lineTo(38, 12.5); ctx.moveTo(10, 20); \
         ctx.lineTo(10, 20); ctx.moveTo(20.5, 18); ctx.lineTo(20.5, 27); ctx.stroke();",
    );
    add(
        "wide dashed lines",
        "ctx.strokeStyle = '#207020'; ctx.lineWidth = 3; ctx.setLineDash([2, 1.5, 0.5, 1]); \
         ctx.lineDashOffset = 1.25; ctx.beginPath(); ctx.moveTo(2, 5); ctx.lineTo(46, 9); \
         ctx.moveTo(4, 14.5); ctx.lineTo(44, 14.5); ctx.moveTo(40, 19); ctx.lineTo(8, 29); ctx.stroke();",
    );
    add(
        "dash offset without dashes",
        "ctx.strokeStyle = '#702070'; ctx.lineWidth = 3; ctx.lineDashOffset = 2; ctx.beginPath(); \
         ctx.moveTo(4, 5); ctx.lineTo(44, 8); ctx.stroke(); ctx.beginPath(); ctx.moveTo(4, 14); \
         ctx.lineTo(24, 28); ctx.lineTo(44, 14); ctx.stroke(); ctx.lineWidth = 1; ctx.beginPath(); \
         ctx.moveTo(4, 30.5); ctx.lineTo(44, 30.5); ctx.stroke();",
    );
    add(
        "wide lines under transform",
        "ctx.translate(24, 16); ctx.rotate(-0.4); ctx.scale(1.5, 1.5); ctx.strokeStyle = '#a08020'; \
         ctx.lineWidth = 2.5; ctx.lineCap = 'square'; ctx.setLineDash([3, 1]); ctx.beginPath(); \
         ctx.moveTo(-12, -6); ctx.lineTo(12, -6); ctx.moveTo(-12, 0); ctx.lineTo(12, 5); ctx.stroke();",
    );
    add(
        "rotated gradient rect",
        "var g = ctx.createLinearGradient(-14, 0, 14, 0); g.addColorStop(0, '#ff8000'); \
         g.addColorStop(1, 'rgba(0, 64, 255, 0.4)'); ctx.translate(24, 16); ctx.rotate(-1.1); \
         ctx.scale(0.8, 0.8); ctx.fillStyle = g; ctx.fillRect(-16, -9, 32, 18); ctx.clearRect(-4, -3, 8, 6);",
    );
    add(
        "clip rect in rect",
        "ctx.beginPath(); ctx.rect(4.4, 3.6, 36, 24); ctx.clip(); ctx.beginPath(); \
         ctx.rect(10.5, 1, 20.2, 40); ctx.clip(); ctx.fillStyle = '#b03060'; ctx.fillRect(0, 0, 48, 32); \
         ctx.strokeStyle = '#000000'; ctx.beginPath(); ctx.moveTo(0, 10.5); ctx.lineTo(48, 10.5); ctx.stroke();",
    );
    add(
        "clip rect then shape",
        "ctx.beginPath(); ctx.rect(6, 5, 30, 20); ctx.clip(); ctx.beginPath(); \
         ctx.ellipse(14, 2, 30, 26); ctx.moveTo(8, 8); ctx.lineTo(20, 8); ctx.lineTo(8, 20); ctx.clip(); \
         ctx.fillStyle = '#3090b0'; ctx.fillRect(0, 0, 48, 32);",
    );
    add(
        "clip shape inside rect",
        "ctx.beginPath(); ctx.rect(2, 2, 44, 28); ctx.clip(); ctx.beginPath(); \
         ctx.roundedRect(8.5, 6.5, 30, 18, 5, 5); ctx.clip(); ctx.fillStyle = '#806020'; \
         ctx.fillRect(0, 0, 48, 32); ctx.beginPath(); ctx.rect(60, 60, 4, 4); ctx.clip(); \
         ctx.fillStyle = '#ff0000'; ctx.fillRect(0, 0, 48, 32);",
    );
    add(
        "clip under scale and translate",
        "ctx.scale(1.5, 1.25); ctx.beginPath(); ctx.rect(3.3, 2.7, 20, 15); ctx.clip(); \
         ctx.translate(5, 0); ctx.fillStyle = '#5050c0'; ctx.fillRect(-10, -10, 60, 60); \
         ctx.beginPath(); ctx.rect(0, 0, 12, 30); ctx.clip(); ctx.fillStyle = '#c0c050'; \
         ctx.fillRect(-10, 6, 60, 5);",
    );
    add(
        "clip under rotation",
        "ctx.translate(24, 16); ctx.rotate(0.3); ctx.beginPath(); ctx.rect(-15, -8, 30, 16); \
         ctx.clip(); ctx.fillStyle = '#408040'; ctx.fillRect(-30, -30, 60, 60);",
    );
    add(
        "clip rule of the path",
        "ctx.save(); ctx.beginPath(); ctx.rect(2, 2, 30, 20); ctx.rect(10, 8, 30, 20); ctx.fill(); \
         ctx.clip(); ctx.fillStyle = '#c03030'; ctx.fillRect(0, 0, 48, 32); ctx.restore(); \
         ctx.beginPath(); ctx.fillRule = 'WindingFill'; ctx.arc(30, 10, 9, 0, 7, false); \
         ctx.arc(36, 12, 9, 0, 7, false); ctx.clip(); ctx.fillStyle = '#3030c0'; ctx.fillRect(0, 0, 48, 32);",
    );
    add(
        "clip two shapes",
        "ctx.beginPath(); ctx.arc(18, 16, 13, 0, 2 * Math.PI, false); ctx.clip(); ctx.beginPath(); \
         ctx.arc(30, 16, 13, 0, 2 * Math.PI, false); ctx.clip(); ctx.fillStyle = '#904090'; \
         ctx.fillRect(0, 0, 48, 32);",
    );
    add(
        "clip triangle and circle",
        "ctx.beginPath(); ctx.moveTo(4, 29); ctx.lineTo(24, 2); ctx.lineTo(45, 27); ctx.closePath(); \
         ctx.clip(); ctx.beginPath(); ctx.arc(26, 20, 12, 0, 2 * Math.PI, false); ctx.clip(); \
         ctx.fillStyle = '#207050'; ctx.fillRect(0, 0, 48, 32);",
    );
    add(
        "clip winding star and ellipse",
        "ctx.beginPath(); ctx.moveTo(24, 1); ctx.lineTo(34, 30); ctx.lineTo(3, 12); ctx.lineTo(45, 12); \
         ctx.lineTo(14, 30); ctx.closePath(); ctx.clip(); ctx.beginPath(); ctx.ellipse(8, 6, 34, 20); \
         ctx.clip(); ctx.fillStyle = '#a06010'; ctx.fillRect(0, 0, 48, 32);",
    );
    add(
        "clip odd-even star and ellipse",
        "ctx.fillRule = Qt.OddEvenFill; ctx.beginPath(); ctx.moveTo(24, 1); ctx.lineTo(34, 30); \
         ctx.lineTo(3, 12); ctx.lineTo(45, 12); ctx.lineTo(14, 30); ctx.closePath(); ctx.clip(); \
         ctx.beginPath(); ctx.ellipse(8, 6, 34, 20); ctx.clip(); ctx.fillStyle = '#a06010'; \
         ctx.fillRect(0, 0, 48, 32);",
    );
    add(
        "clip three shapes",
        "ctx.beginPath(); ctx.arc(18, 12, 12, 0, 2 * Math.PI, false); ctx.clip(); ctx.beginPath(); \
         ctx.arc(30, 12, 12, 0, 2 * Math.PI, false); ctx.clip(); ctx.beginPath(); \
         ctx.arc(24, 21, 12, 0, 2 * Math.PI, false); ctx.clip(); ctx.fillStyle = '#5030b0'; \
         ctx.fillRect(0, 0, 48, 32);",
    );
    add(
        "clip shapes sharing an edge",
        "ctx.beginPath(); ctx.moveTo(4, 4); ctx.lineTo(40, 4); ctx.lineTo(30, 28); ctx.lineTo(4, 20); \
         ctx.closePath(); ctx.clip(); ctx.beginPath(); ctx.moveTo(10, 4); ctx.lineTo(44, 4); \
         ctx.lineTo(44, 16); ctx.lineTo(12, 30); ctx.closePath(); ctx.clip(); ctx.fillStyle = '#b02060'; \
         ctx.fillRect(0, 0, 48, 32);",
    );
    add(
        "clip shape inside shape",
        "ctx.beginPath(); ctx.arc(24, 16, 14, 0, 2 * Math.PI, false); ctx.clip(); ctx.beginPath(); \
         ctx.moveTo(24, 6); ctx.lineTo(32, 20); ctx.lineTo(17, 22); ctx.closePath(); ctx.clip(); \
         ctx.fillStyle = '#306090'; ctx.fillRect(0, 0, 48, 32);",
    );
    add(
        "clip shapes apart with bounds that overlap",
        "ctx.fillStyle = '#808080'; ctx.fillRect(0, 0, 6, 6); ctx.beginPath(); ctx.moveTo(2, 2); \
         ctx.lineTo(30, 2); ctx.lineTo(2, 24); ctx.closePath(); ctx.clip(); ctx.beginPath(); \
         ctx.moveTo(46, 30); ctx.lineTo(46, 12); ctx.lineTo(22, 30); ctx.closePath(); ctx.clip(); \
         ctx.fillStyle = '#c04040'; ctx.fillRect(0, 0, 48, 32);",
    );
    add(
        "clip ring and slanted bar",
        "ctx.fillRule = Qt.OddEvenFill; ctx.beginPath(); ctx.arc(24, 16, 14, 0, 2 * Math.PI, false); \
         ctx.closePath(); ctx.arc(24, 16, 7, 0, 2 * Math.PI, false); ctx.clip(); ctx.beginPath(); \
         ctx.moveTo(2, 20); ctx.lineTo(44, 4); ctx.lineTo(46, 12); ctx.lineTo(5, 29); ctx.closePath(); \
         ctx.clip(); ctx.fillStyle = '#208080'; ctx.fillRect(0, 0, 48, 32);",
    );
    add(
        "clip two shapes under a transform",
        "ctx.translate(24, 16); ctx.rotate(0.4); ctx.scale(1.3, 0.8); ctx.beginPath(); \
         ctx.roundedRect(-15, -12, 30, 24, 7, 9); ctx.clip(); ctx.rotate(-0.9); ctx.beginPath(); \
         ctx.ellipse(-18, -7, 36, 14); ctx.clip(); ctx.fillStyle = '#707020'; \
         ctx.fillRect(-40, -40, 80, 80);",
    );
    add(
        "clip two shapes, gradient and stroke through them",
        "ctx.beginPath(); ctx.arc(17, 16, 14, 0, 2 * Math.PI, false); ctx.clip(); ctx.beginPath(); \
         ctx.moveTo(8, 2); ctx.bezierCurveTo(50, 0, 50, 30, 10, 30); ctx.quadraticCurveTo(26, 16, 8, 2); \
         ctx.clip(); var g = ctx.createLinearGradient(0, 0, 48, 32); g.addColorStop(0, '#ff0000'); \
         g.addColorStop(1, '#0000ff'); ctx.fillStyle = g; ctx.fillRect(0, 0, 48, 32); \
         ctx.strokeStyle = '#ffffff'; ctx.lineWidth = 3; ctx.beginPath(); ctx.moveTo(0, 30); \
         ctx.lineTo(48, 4); ctx.stroke();",
    );
    add(
        "clip two shapes, then restore",
        "ctx.save(); ctx.beginPath(); ctx.arc(18, 16, 13, 0, 2 * Math.PI, false); ctx.clip(); \
         ctx.save(); ctx.beginPath(); ctx.arc(30, 16, 13, 0, 2 * Math.PI, false); ctx.clip(); \
         ctx.fillStyle = '#904090'; ctx.fillRect(0, 0, 48, 32); ctx.restore(); \
         ctx.fillStyle = 'rgba(0, 160, 0, 0.5)'; ctx.fillRect(0, 0, 48, 12); ctx.restore(); \
         ctx.fillStyle = 'rgba(0, 0, 200, 0.5)'; ctx.fillRect(0, 26, 48, 6);",
    );
    add(
        "line stroked across a rect clip",
        "ctx.beginPath(); ctx.rect(10, 2, 21, 28); ctx.clip(); ctx.fillStyle = '#000000'; \
         ctx.fillRect(0, 0, 48, 32); ctx.strokeStyle = '#ffffff'; ctx.lineWidth = 3; ctx.beginPath(); ctx.moveTo(0, 30); \
         ctx.lineTo(48, 4); ctx.stroke();",
    );
    add(
        "line stroked across a triangle clip",
        "ctx.beginPath(); ctx.moveTo(10, 2); ctx.lineTo(31, 16); ctx.lineTo(10, 30); ctx.clip(); \
         ctx.fillStyle = '#000000'; ctx.fillRect(0, 0, 48, 32); ctx.strokeStyle = '#ffffff'; ctx.lineWidth = 3; ctx.beginPath(); ctx.moveTo(0, 30); \
         ctx.lineTo(48, 4); ctx.stroke();",
    );
    add(
        "dashed line and rect filled across a clip",
        "ctx.beginPath(); ctx.moveTo(12, 3); ctx.lineTo(40, 14); ctx.lineTo(9, 29); ctx.clip(); \
         ctx.rotate(0.2); ctx.fillStyle = '#204080'; ctx.fillRect(4.5, -3.25, 40, 22.5); \
         ctx.strokeStyle = '#ffe080'; ctx.lineWidth = 2.5; ctx.setLineDash([3, 2]); ctx.beginPath(); \
         ctx.moveTo(2, 3); ctx.lineTo(46, 20); ctx.stroke();",
    );
    add(
        "chaining",
        "ctx.beginPath().moveTo(4, 4).lineTo(40, 4).lineTo(20, 28).closePath().fill(); \
         return String(ctx.save() === ctx) + String(ctx.restore() === ctx);",
    );
    v
}

const HEX: &str = "function hex(d) { var h = '0123456789abcdef', s = ''; \
    for (var i = 0; i < d.length; i++) s += h[d[i] >> 4] + h[d[i] & 15]; return s; }";

/// The cases that still differ from Qt: none.
const KNOWN: &[&str] = &[];

/// One Canvas whose first paint runs every case from a clean context.
fn source(tail: &str) -> String {
    let fns: Vec<String> = cases()
        .iter()
        .map(|(_, b)| format!("function(ctx) {{ {b} }}"))
        .collect();
    format!(
        "import QtQuick\nItem {{\n    id: root\n    width: 200; height: 200\n    property var results: []\n    \
         property bool done: false\n    Canvas {{\n        id: cv\n        width: {CW}; height: {CH}\n        \
         renderTarget: Canvas.Image; renderStrategy: Canvas.Immediate\n        \
         property var fns: [{}]\n        onPaint: {{\n            if (root.done) return;\n            \
         root.done = true;\n            {HEX}\n            var ctx = getContext('2d');\n            \
         var out = [];\n            for (var i = 0; i < fns.length; i++) {{\n                \
         ctx.reset();\n                var r;\n                \
         try {{ r = fns[i](ctx); }} catch (e) {{ r = 'EXC ' + e.message; }}\n                \
         out.push(hex(ctx.getImageData(0, 0, {CW}, {CH}).data) + '|' + r);\n            }}\n            \
         ctx.reset();\n            {tail}\n        }}\n    }}\n}}\n",
        fns.join(",\n            ")
    )
}

fn qt_results() -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("canvas.qml");
    let src =
        source("for (var k = 0; k < out.length; k++) console.log('CASE', k, out[k]); Qt.quit();");
    std::fs::write(&file, src).unwrap();
    let out = std::process::Command::new("timeout")
        .args(["120", "qml6"])
        .arg(&file)
        .env("QT_QPA_PLATFORM", "offscreen")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    let res: Vec<String> = log
        .lines()
        .filter_map(|l| l.split_once("CASE ")?.1.split_once(' '))
        .map(|(_, r)| r.to_string())
        .collect();
    let short: String = log
        .lines()
        .map(|l| &l[..l.len().min(200)])
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(res.len(), cases().len(), "qml6 printed:\n{short}");
    res
}

fn our_results() -> Vec<String> {
    // One onPaint runs every case and turns each image into hex in JS: more than the
    // watchdog's five seconds of CPU in a debug build.
    qml::watchdog::set_timeout(None);
    let mut rt = qml::runtime::Runtime::new(None);
    let root = rt.load_str(&source("root.results = out;")).expect("loads");
    let mut fonts = scene::Fonts::default();
    let mut c = Canvas::new(200, 200, [0, 0, 0, 0]);
    for _ in 0..3 {
        rt.settle();
        scene::layout_window(&mut rt, &mut fonts, root, [200.0, 200.0]);
        rt.canvases_available();
        rt.settle();
        scene::paint_window(&mut rt, &mut fonts, root, &mut Target::Cpu(&mut c));
    }
    rt.settle();
    let mut w = rt.world.borrow_mut();
    assert!(w.errors.is_empty(), "script errors: {:?}", w.errors);
    match w.read(root, "results") {
        Some(qml::value::Value::Obj(o)) => o
            .borrow()
            .array
            .as_ref()
            .map(|a| a.iter().map(|v| v.to_js_string()).collect())
            .unwrap_or_default(),
        _ => vec![],
    }
}

/// (differing pixels, largest channel difference) of two hex RGBA dumps.
fn pixel_diff(a: &str, b: &str) -> (usize, u32) {
    // The canvas image is premultiplied; getImageData's division by alpha is rounded by
    // Qt's SSE code one way or the other at exact halves, so compare what is on the image.
    let px = |s: &str| -> Vec<u32> {
        (0..s.len() / 8)
            .map(|i| {
                let v = u32::from_str_radix(&s[8 * i..8 * i + 8], 16).unwrap_or(0);
                let a = v & 255;
                let pm = |c: u32| mul255::qt(c, a);
                pm(v >> 24) << 24 | pm((v >> 16) & 255) << 16 | pm((v >> 8) & 255) << 8 | a
            })
            .collect()
    };
    let (a, b) = (px(a), px(b));
    if a.len() != b.len() {
        return (a.len().max(b.len()), 255);
    }
    let (mut n, mut worst) = (0, 0);
    for (x, y) in a.iter().zip(&b) {
        if x != y {
            n += 1;
            for sh in [0, 8, 16, 24] {
                worst = worst.max(((x >> sh) & 255).abs_diff((y >> sh) & 255));
            }
        }
    }
    (n, worst)
}

#[test]
fn context2d_draws_and_answers_as_qt() {
    if !testenv::require("qml6") {
        return;
    }
    let (qt, ours) = (qt_results(), our_results());
    assert_eq!(ours.len(), qt.len(), "our onPaint ran every case");
    let mut report = String::new();
    let mut bad = 0;
    let mut differing: Vec<String> = Vec::new();
    for (((name, _), q), o) in cases().iter().zip(&qt).zip(&ours) {
        let (qp, qr) = q.split_once('|').unwrap_or((q.as_str(), ""));
        let (op, or) = o.split_once('|').unwrap_or((o.as_str(), ""));
        let (n, worst) = pixel_diff(qp, op);
        if let Ok(dir) = std::env::var("C2D_DUMP") {
            let i = cases().iter().position(|(c, _)| c == name).unwrap();
            let _ = std::fs::write(format!("{dir}/{i:02}.qt"), q);
            let _ = std::fs::write(format!("{dir}/{i:02}.ours"), o);
        }
        let drawn = qp
            .as_bytes()
            .chunks(8)
            .filter(|c| *c != b"00000000")
            .count();
        if n != 0 || qr != or {
            bad += 1;
            differing.push(name.clone());
            report += &format!(
                "{name}: {n} of {} pixels differ (largest channel difference {worst}, Qt drew {drawn})",
                CW * CH
            );
            if qr != or {
                report += &format!("; Qt returned {qr:?}, we returned {or:?}");
            }
            report += "\n";
        }
    }
    println!("{} cases, {bad} differ from Qt\n{report}", qt.len());
    assert_eq!(
        differing,
        KNOWN,
        "{bad} of {} cases differ from Qt, {} of them known:\n{report}",
        qt.len(),
        KNOWN.len()
    );
}

fn paint(qml: &str, w: usize, h: usize) -> Canvas {
    let mut rt = qml::runtime::Runtime::new(None);
    let win = rt.load_str(qml).expect("scene loads");
    let mut fonts = scene::Fonts::default();
    let mut c = Canvas::new(w, h, [0, 0, 0, 0]);
    for _ in 0..3 {
        rt.settle();
        scene::layout_window(&mut rt, &mut fonts, win, [w as f64, h as f64]);
        // The frame's polish: canvases start painting once laid out.
        rt.canvases_available();
        rt.settle();
        scene::paint_window(&mut rt, &mut fonts, win, &mut Target::Cpu(&mut c));
    }
    let errors = rt.world.borrow().errors.clone();
    assert!(errors.is_empty(), "script errors: {errors:?}");
    c
}

/// AcrylicRectangle.qml's onPaint (waffle looks): the 1px edge line along the top, a gradient
/// stroke through two arcTo corners, drawn after clearRect.
#[test]
fn acrylic_edge_line_is_drawn_after_clear_rect() {
    let c = paint(
        "import QtQuick\nimport Quickshell\nFloatingWindow {\n    width: 80; height: 40\n    color: \"#000000\"\n    \
         Canvas {\n        anchors.fill: parent\n        property color borderColor: \"#ffffff\"\n        \
         onPaint: {\n            var ctx = getContext(\"2d\");\n            var r = 8;\n            \
         ctx.clearRect(0, 0, width, height);\n            \
         var grad = ctx.createLinearGradient(0, 0, width, 0);\n            \
         grad.addColorStop(0, Qt.rgba(borderColor.r, borderColor.g, borderColor.b, 0));\n            \
         grad.addColorStop(0.2, borderColor);\n            grad.addColorStop(0.8, borderColor);\n            \
         grad.addColorStop(1, Qt.rgba(borderColor.r, borderColor.g, borderColor.b, 0));\n            \
         ctx.strokeStyle = grad;\n            ctx.lineWidth = 1;\n            ctx.beginPath();\n            \
         ctx.moveTo(r, 0.5);\n            ctx.lineTo(width - r, 0.5);\n            \
         ctx.arcTo(width, 0.5, width, r + 0.5, r);\n            ctx.moveTo(width - r, 0.5);\n            \
         ctx.arcTo(0, 0.5, 0, r + 0.5, r);\n            ctx.stroke();\n        }\n    }\n}\n",
        80,
        40,
    );
    let at = |x: usize, y: usize| c.px[y * 80 + x];
    assert_eq!(
        at(40, 0),
        [255, 255, 255, 255],
        "the edge line in the middle of the top row"
    );
    assert_eq!(at(40, 20), [0, 0, 0, 255], "nothing below the line");
    assert!(
        at(77, 3)[0] > 0,
        "the top right corner arc: {:?}",
        at(77, 3)
    );
    assert!(
        at(12, 0)[0] > 100 && at(12, 0)[0] < 255,
        "the line fades in: {:?}",
        at(12, 0)
    );
}

/// Non-finite arguments are ignored as in Qt: the drawing is that of the finite calls, and a
/// dashed stroke after `scale(NaN, 1)` returns (it used to never finish).
#[test]
fn non_finite_arguments_are_ignored_and_cannot_hang() {
    let c = paint(
        "import QtQuick\nimport Quickshell\nFloatingWindow {\n    width: 80; height: 40\n    color: \"#000000\"\n    \
         Canvas {\n        anchors.fill: parent\n        onPaint: {\n            \
         var ctx = getContext(\"2d\");\n            ctx.scale(NaN, 1);\n            \
         ctx.translate(Infinity, 0);\n            ctx.rotate(NaN);\n            \
         ctx.strokeStyle = \"#ffffff\";\n            ctx.lineWidth = 2;\n            \
         ctx.setLineDash([4, 4]);\n            ctx.beginPath();\n            \
         ctx.moveTo(NaN, 10);\n            ctx.moveTo(0, 10);\n            \
         ctx.lineTo(Infinity, 10);\n            ctx.lineTo(80, 10);\n            ctx.stroke();\n        \
         }\n    }\n}\n",
        80,
        40,
    );
    let px = &c.px;
    let at = |x: usize, y: usize| px[y * 80 + x];
    // lineWidth 2, dashes of 4 widths: 8px on, 8px off along y = 10.
    assert_eq!(at(4, 10), [255, 255, 255, 255], "first dash");
    assert_eq!(at(12, 10), [0, 0, 0, 255], "first gap");
    assert_eq!(at(20, 10), [255, 255, 255, 255], "second dash");
    assert_eq!(at(4, 20), [0, 0, 0, 255], "nothing else is drawn");
}

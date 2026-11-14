import QtQuick
import Impulse
import "corner-rounding.js" as CR
import "offset.js" as Off

QtObject {
    // Every Material shape and the polygons MaterialCookie builds: their cubics, features and
    // bounds, the matrices and roundings, and the morph of every pair at four progress values.
    function shapes() {
        const MS = MaterialShapes, R = RoundedPolygon;
        var out = [];
        var num = x => (x === 0 && 1 / x < 0) ? '-0' : String(x);
        var nums = a => a.map(num).join(',');
        var gs = ['getCircle', 'getSquare', 'getSlanted', 'getArch', 'getFan', 'getArrow', 'getSemiCircle', 'getOval', 'getPill', 'getTriangle', 'getDiamond', 'getClamShell', 'getPentagon', 'getGem', 'getSunny', 'getVerySunny', 'getCookie4Sided', 'getCookie6Sided', 'getCookie7Sided', 'getCookie9Sided', 'getCookie12Sided', 'getGhostish', 'getClover4Leaf', 'getClover8Leaf', 'getBurst', 'getSoftBurst', 'getBoom', 'getSoftBoom', 'getFlower', 'getPuffy', 'getPuffyDiamond', 'getPixelCircle', 'getPixelTriangle', 'getBun', 'getHeart'];
        var polys = gs.map(k => MS[k]());
        out.push('cached ' + gs.every(k => MS[k]() === MS[k]()));
        for (const s of [3, 5, 8, 10, 15, 20]) polys.push(R.star(s, 1, 0.8, new CR.CornerRounding((s < 17 ? 1.5 : 1.1) / Math.max(s, 1))).transformed((x, y) => MS.rotate30.map(new Off.Offset(x, y))).normalized());
        var extra = [
          R.star(5, 1, 0.4, new CR.CornerRounding(0.1), new CR.CornerRounding(0.5), null, 0.5, 0.5), R.star(4.5, 2, 0.3),
          MS.customPolygon([MS.pointNRound(new Off.Offset(1, 0.67), new CR.CornerRounding(0.211, 2)), MS.pointNRound(new Off.Offset(0, 0.67), new CR.CornerRounding(0.207, 2)), MS.pointNRound(new Off.Offset(0, 0), new CR.CornerRounding(0.207, 2)), MS.pointNRound(new Off.Offset(1, 0), new CR.CornerRounding(0.211, 2))]),
          MS.customPolygon([MS.pointNRound(new Off.Offset(0.9, 0.6), new CR.CornerRounding(0.1)), MS.pointNRound(new Off.Offset(0.7, 0.95))], 5, new Off.Offset(0.5, 0.5), true),
          MS.customPolygon([MS.pointNRound(new Off.Offset(0.8, 0.5)), MS.pointNRound(new Off.Offset(0.6, 0.9), new CR.CornerRounding(0.2, 0.3))], 3, new Off.Offset(0.4, 0.6)),
          MS.getHeart().transformed((x, y) => ({x: x * 2 - y, y: y + 0.25})), MS.getSquare().transformed((x, y) => MS.rotateNeg135.map({x: x, y: y})).normalized()];
        for (const p of polys.concat(extra)) {
          out.push('P ' + num(p.centerX) + ',' + num(p.centerY) + ' c' + nums([p.center.x, p.center.y]) + ' ' + p.cubics.map(c => nums(c.points)).join('|'));
          out.push('F ' + p.features.map(f => f.isEdge + ':' + f.convex + ':' + f.isConvexCorner + ':' + f.isConcaveCorner + ':' + f.cubics.map(c => nums(c.points) + ';' + num(c.anchor1X)).join('/')).join('|'));
          const b = [7, 7, 7, 7, 7];
          out.push('B ' + nums(p.calculateBounds()) + ' ' + nums(p.calculateBounds([], false)) + ' ' + nums(p.calculateMaxBounds()) + ' ' + nums(p.calculateBounds(b)) + ' ' + nums(b) + ' ' + nums(p.buildCubicList().map(c => c.points[3])));
        }
        out.push('M ' + ['rotateNeg30', 'rotateNeg45', 'rotateNeg90', 'rotateNeg135', 'rotate30', 'rotate45', 'rotate60', 'rotate90', 'rotate120', 'rotate135', 'rotate180', 'rotate28th', 'rotateNeg16th'].map(k => { const q = MS[k].map(new Off.Offset(0.3, -0.7)); return nums([q.x, q.y]); }).join('|'));
        out.push('R ' + [MS.cornerRound15, MS.cornerRound20, MS.cornerRound30, MS.cornerRound50, MS.cornerRound100].map(r => nums([r.radius, r.smoothing])).join('|'));
        var ps = [0, 0.37, 0.999, 1.3];
        for (let i = 0; i < polys.length; i++) for (let j = 0; j < polys.length; j++) {
          try { const m = Morph.create(polys[i], polys[j]); for (const p of ps) { const cs = m.asCubics(p);
            out.push(cs.length + ':' + cs.map(c => c.points.join(',') + ';' + c.anchor0X + ',' + c.anchor1Y + ',' + c.control1X).join('|')); } }
          catch (e) { out.push('E' + e.name); }
        }
        for (const bad of [null, undefined]) { try { Morph.create(bad, polys[0]); out.push('ok'); } catch (e) { out.push('E' + e.name); } }
        var res = gs.length + ' shapes\n' + out.join('\n');
        return res;
    }

    // Searches over application-like names (prepared, raw, nested keys, limits, thresholds,
    // highlighting) and the edit-distance scores of every pair of names.
    function libs() {
        const Fuzzy = FuzzySort, Levendist = Levenshtein;
        var out = [];
        var names = ['firefox', 'Firefox Web Browser', 'Visual Studio Code', 'GNU Image Manipulation Program', 'LibreOffice Writer',
          'LibreOffice Calc', 'Café Crème', 'Ångström Units', 'naïve résumé', 'Straße', 'İstanbul', 'ΑΘΗΝΑ Αθήνα', '日本語入力 Mozc',
          'emoji 😀 grinning face', 'org.gnome.Nautilus', 'kitty', 'htop', 'CheatManager.h', 'Manifest.cpp', 'strawberry',
          'straw berry jam', 'a-b-c-d-e-f-g-h-i-j-k-l-m-n-o-p-q-r-s-t-u-v-w-x-y-z-0-1-2-3', 'XMLHttpRequest', 'camelCaseWordsHere',
          'snake_case_words_here', 'Files', 'Settings', 'System Monitor', 'Text Editor', 'Terminal', 'Thunar File Manager',
          '  padded name  ', 'tab\tseparated', 'ǅungla Ǆ', 'ﬁle ligature', 'Ｆｕｌｌｗｉｄｔｈ', 'x'.repeat(1001) + 'firefox',
          'https://github.com/end-4/dots-hyprland/issues/123', 'mpv Media Player', 'Steam (Runtime)', 'OBS Studio', 'Obsidian',
          'zathura', 'Zen Browser', 'Brave Web Browser', 'Chromium', 'Discord', 'Telegram Desktop', 'Blender 4.2', 'v1.2.3-rc4'];
        var targets = names.map((n, i) => ({ id: i, name: Fuzzy.prepare(n + ' ') }));
        targets.push({ id: 'num', name: Fuzzy.prepare(42) }, { id: 'obj', name: Fuzzy.prepare({}) }, { id: 'none', name: Fuzzy.prepare() });
        var raw = names.map((n, i) => ({ id: 'raw' + i, name: n }));
        raw.push({ id: 'n', name: 1234 }, { id: 'e', name: '' }, { id: 'u' }, { id: 'nest', name: 'x', deep: { k: 'Deep Firefox Key' } });
        var qs = ['', 'f', 'fire', 'Firefox', 'vsc', 'code', 'gimp', 'cafe', 'café', 'résumé', 'resume', 'c man', 'straw berry', 'str',
          '  spaced  out ', 'ist', 'istanbul', 'αθη', 'Αθήνα', '日本', '😀', 'xyzzy', 'o', 'e', 'li wr', 'a b c', 'b a', '42', 'NAUT',
          'gnome nautilus', 'mani cpp', 'xml http', 'ccwh', 'sch', 'file', 'fi', 'ﬁ', 'fw', 'ｆｕｌ', 'term', 'te ed', 'sys mon',
          'z', 'zz', 'y-z', '0123', 'abcdefghijklmnopqrstuvwxyz', 'issues 123', 'dots hypr', 'web web', 'web brow', 'b b b', '1.2',
          'x'.repeat(30), 'blender 4', 'steam runtime', '(', 'ǆ', 'straße', 'STRASSE', 'tab sep', 'pad'];
        var num = x => (x === 0 && 1 / x < 0) ? '-0' : String(x);
        var show = r => r === null ? 'null' : [JSON.stringify(r.target).slice(0, 80), r.obj === undefined ? 'U' : (r.obj === null ? 'N' : r.obj.id),
          num(r.score), num(r._score), r.indexes.join(','), r.highlight().slice(0, 120), r.highlight('[', ']').slice(0, 120),
          JSON.stringify(r.highlight((m, i) => i + ':' + m)).slice(0, 160)].join('~');
        for (const q of qs) {
          const rs = Fuzzy.go(q, targets, { all: true, key: 'name' });
          out.push('Q ' + JSON.stringify(q) + ' ' + rs.length + '/' + rs.total);
          for (const r of rs) out.push(' ' + show(r));
          const r2 = Fuzzy.go(q, raw, { key: 'name', limit: 3, threshold: 0.3 });
          out.push(' L ' + r2.length + '/' + r2.total + ' ' + r2.map(show).join(' | '));
          const r3 = Fuzzy.go(q, raw, { key: 'deep.k' });
          out.push(' D ' + r3.length + '/' + r3.total + ' ' + r3.map(show).join(' | '));
          const r4 = Fuzzy.go(q, names.slice(0, 20), { limit: 5 });
          out.push(' N ' + r4.length + '/' + r4.total + ' ' + r4.map(show).join(' | '));
          const r5 = Fuzzy.go(q, names.slice(0, 10).map(n => Fuzzy.prepare(n)), { all: true });
          out.push(' A ' + r5.length + '/' + r5.total + ' ' + r5.map(show).join(' | '));
          out.push(' P ' + targets.map(t => num(t.name._score) + ':' + t.name.indexes.join('.')).join(' '));
        }
        var lower = names.slice(0, 40).map(n => n.toLowerCase()).concat(['', 'a', 'ab', 'ba']);
        for (const a of lower) {
          const row = [];
          for (const b of lower) row.push(num(Levendist.computeScore(a, b)) + ',' + num(Levendist.computeTextMatchScore(a, b)));
          out.push('V ' + row.join(' '));
        }
        var res = out.join('\n');
        return res;
    }
}

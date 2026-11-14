import sys, re, collections
path, kind = sys.argv[1], sys.argv[2]
Q = [
 ('gc',        r'QV4::MemoryManager|GCStateMachine|QV4::.*::markObjects|QV4::Heap::.*mark'),
 ('json',      r'JsonObject|Stringify|JsonParser'),
 ('finalize',  r'QQmlObjectCreator::finalize|componentComplete|QQmlComponentPrivate::complete|completeCreate|forceCompletion'),
 ('create',    r'QQmlObjectCreator::|QQmlComponent::create|QQmlComponentPrivate::beginCreate|QQmlIncubatorPrivate::incubate|QQmlDelegateModel|QQmlTableInstanceModel|QQmlObjectModel|QQuickRepeater|Instantiator|QQmlType::create|QQmlListModel|~QQuick|~QObject|deleteLater|QObjectPrivate::deleteChildren'),
 ('binding',   r'QQmlBinding::|QQmlPropertyBinding|QPropertyBindingPrivate'),
 ('handler',   r'QQmlBoundSignalExpression|QQmlJavaScriptExpression|QV4::|memfd:JITCode|perf-\d+\.map|QJSValue|QJSEngine|QQmlVMEMetaObject'),
 ('layout',    r'polishItems|updatePolish|QQuickItemView|QQuickListView|QQuickLayout|QQuickGridLayout|QQuickFlickable|QQuickBasePositioner|QTextLayout|QQuickText|QQuickAnchor|QQuickItemPrivate::|QQuickItem::set|QFont|freetype|harfbuzz'),
 ('render',    r'syncSceneGraph|renderSceneGraph|QSG|QRhi|updatePaintNode|libgallium|swrast|libGLX|libEGL|libGL|QQuickWindowPrivate::sync|libvulkan|wl_|libwayland|QtWayland'),
 ('dbus',      r'QDBus|libdbus'),
]
N = [
 ('json',      r'json_'),
 ('finalize',  r'finalize_settle'),
 ('create',    r'create_object|::instantiate|update_delegators|World>::destroy'),
 ('binding',   r'eval_binding|flush_sync'),
 ('handler',   r'run_handler|run_own_handler|call_compiled|Interp>::call|ricompiled'),
 ('layout',    r'relayout|layout_window|layout_layer|qml::layout::|shell::text|qsg::text|font|swash|harfbuzz|rustybuzz'),
 ('render',    r'App>::draw|qsg::|shell::gpu|shell::scene|libgallium|swrast|libGL|libEGL|wl_|libwayland|wayland'),
 ('dbus',      r'zbus|dbus|notifications'),
]
R = [(n, re.compile(p)) for n, p in (Q if kind == 'QT' else N)]
X = {'array': r'ArrayPrototype::method_|jsrt::array|array_(map|filter|forEach|some|find|push|slice)|Array.*(map|filter|for_each)',
     'stringify(any)': r'JsonObject::method_stringify|Stringify|json_write|json_str',
     'polish': r'polishItems', 'syncSceneGraph': r'syncSceneGraph', 'incubate': r'QQmlIncubatorPrivate::incubate',
     'App::draw': r'App>::draw', 'settle': r'Eng>::settle'}
X = {k: re.compile(v) for k, v in X.items()}
def grp(comm):
    if comm.startswith('llvmpipe'): return 'llvmpipe'
    if comm in ('quickshell', 'qs'): return 'main'
    if comm.startswith('QSGRender'): return 'renderthread'
    if comm in ('cliphist', 'notify-send', 'bash'): return 'child-procs'
    return 'other-threads'
ph = collections.Counter(); g = collections.Counter(); x = collections.Counter(); tot = 0
selfs = collections.defaultdict(collections.Counter)
def flush(head, fr):
    global tot
    if not head: return
    tot += 1; gg = grp(head.split()[0]); g[gg] += 1
    if gg == 'main':
        for n, rx in R:
            if any(rx.search(f) for f in fr): ph[n] += 1; selfs[n][fr[0][:80] if fr else '?'] += 1; break
        else: ph['other'] += 1; selfs['other'][fr[0][:80] if fr else '?'] += 1
        for k, rx in X.items():
            if any(rx.search(f) for f in fr): x[k] += 1
head = None; fr = []
for line in open(path, errors='replace'):
    if not line.strip(): flush(head, fr); head = None; fr = []; continue
    if line[0] in ' \t':
        p = line.strip().split(None, 1); fr.append(p[1] if len(p) > 1 else p[0])
    else: head = line
flush(head, fr)
print(f'{kind} total samples {tot}')
for k, v in g.most_common(): print(f'  thread {k:14s} {v*100/tot:5.1f}%')
for n, _ in R + [('other', 0)]: print(f'  main {n:9s} {ph[n]*100/tot:5.1f}% of all')
for k, v in x.items(): print(f'  incl(main) {k:15s} {v*100/tot:5.1f}%')
if '-v' in sys.argv:
    for n in [n for n, _ in R] + ['other']:
        print('---', n, ' '.join(f'[{v}] {k}' for k, v in selfs[n].most_common(6)))

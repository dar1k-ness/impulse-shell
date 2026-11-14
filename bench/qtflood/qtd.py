import sys, re, collections, xml.etree.ElementTree as ET
K = int(sys.argv[2]) if len(sys.argv) > 2 else 300
ev = {}; ranges = []
for _, el in ET.iterparse(sys.argv[1], events=('end',)):
    if el.tag == 'event':
        ev[int(el.get('index'))] = {c.tag: (c.text or '') for c in el}; el.clear()
    elif el.tag == 'range':
        ranges.append((int(el.get('startTime')), int(el.get('duration') or 0), int(el.get('eventIndex')))); el.clear()
ranges.sort()
cnt = collections.Counter(); dur = collections.Counter(); top = collections.Counter()
per = collections.Counter(); perd = collections.Counter()
stack = []  # end times for nesting (QML ranges are single-thread, nested)
qml = {'Creating','Binding','HandlingSignal','Javascript','Compiling'}
for s, d, i in ranges:
    e = ev.get(i, {}); t = e.get('type', '?')
    if t == 'SceneGraph' or t == 'Event' or t == 'MemoryAllocation': key = t + ':' + e.get('displayname', '')
    else: key = t
    cnt[key] += 1; dur[key] += d
    if t in qml:
        while stack and stack[-1] <= s: stack.pop()
        if not stack: top[t] += d
        stack.append(s + d)
        fn = e.get('filename', '').split('/')[-1]
        k = f"{t} {fn}:{e.get('line')} {e.get('details','')}"
        per[k] += 1; perd[k] += d
span = (ranges[-1][0] - ranges[0][0]) / 1e6 if ranges else 0
print(f'ranges {len(ranges)} span {span:.0f} ms, K={K}')
for k in sorted(cnt, key=lambda k: -dur[k]):
    print(f'{k:40s} n={cnt[k]:8d} ({cnt[k]/K:8.1f}/notif) incl {dur[k]/1e6:9.1f} ms')
print('top-level (exclusive of nesting) QML time:', {k: round(v/1e6,1) for k, v in top.items()})
print('--- top items by count')
for k, v in per.most_common(40): print(f'{v:7d} {v/K:7.1f}/n {perd[k]/1e6:8.1f}ms {k}')
print('--- watched')
for k, v in per.items():
    if re.search(r'NotificationItem.qml:(99|16)\b|NotificationGroup.qml:248|groupsForList|stringifyList|Notifications.qml:(9[0-9]|1[0-4][0-9])\b', k):
        print(f'{v:7d} {v/K:7.1f}/n {perd[k]/1e6:8.1f}ms {k}')

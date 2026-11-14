"""gen.py OUT N SEED: N documents, each a seed with a few of its tokens changed."""
import os, random, re, sys
out, n, seed = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
rnd = random.Random(seed)
here = os.path.dirname(os.path.abspath(__file__))
seeds = [open(os.path.join(here, "seeds", f)).read() for f in sorted(os.listdir(os.path.join(here, "seeds")))]
TOKEN = re.compile(r'''\s+|//[^\n]*|/\*.*?\*/|"(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*'|`[^`]*`|[A-Za-z_$][A-Za-z0-9_$]*|\d[\w.]*|>>>=|\.\.\.|===|!==|\*\*=|<<=|>>=|>>>|\?\?=|=>|==|!=|<=|>=|&&|\|\||\?\?|\?\.|\+\+|--|\+=|-=|\*=|/=|%=|&=|\|=|\^=|<<|>>|\*\*|.''', re.S)
POOL = """{ } ( ) [ ] ; , . : ? = + - * / < > ! ~ & | ^ % => ... ?. ?? ++ -- == === += && || @ # ` ' " \\ \n
property signal function readonly required default final virtual override component enum alias var let const
if else for while do switch case break continue return throw try catch finally new delete typeof void in of
instanceof this null true false undefined class extends super static get set yield async await import export
pragma as on id list int Item QtObject Rectangle item x Foo foo.bar Foo.Bar 0 1 1.5 0x 1e 1_ 08 "s" 's' `t` /re/g
with debugger from interface enum public private package 255 256 -1 2.300 $ _ é Ж""".split(" ")
POOL = [t for line in POOL for t in line.split("\n") if t] + ["\n", " "]
def mutate(src):
    toks = TOKEN.findall(src)
    idx = [i for i, t in enumerate(toks) if not t.isspace()]
    for _ in range(rnd.choice([1, 1, 1, 2, 3])):
        i = rnd.choice(idx)
        op = rnd.randrange(7)
        if op == 0:
            toks[i] = ""
        elif op == 1:
            toks[i] = toks[i] + " " + toks[i]
        elif op == 2:
            toks[i] = rnd.choice(POOL)
        elif op == 3:
            toks[i] = rnd.choice(POOL) + " " + toks[i]
        elif op == 4:
            j = rnd.choice(idx)
            toks[i], toks[j] = toks[j], toks[i]
        elif op == 5:
            toks[i] = toks[i] + "\n"
        else:
            j = rnd.choice(idx)
            toks[i] = toks[j]
    return "".join(toks)
os.makedirs(out, exist_ok=True)
for k in range(n):
    open(os.path.join(out, f"G{k:05}.qml"), "w").write(mutate(rnd.choice(seeds)))

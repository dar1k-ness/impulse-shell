"rel.html"	string	true	1|file://DIR/rel.html
"rel.html"	url	true	1|file://DIR/rel.html
"rel.html"	resolved	true	1|file://DIR/rel.html
"sub/a b.html"	string	true	1|file://DIR/sub/a%20b.html
"sub/a b.html"	url	true	1|file://DIR/sub/a%20b.html
"sub/a b.html"	resolved	true	1|file://DIR/sub/a%20b.html
"sub/a%20b.html"	string	true	1|file://DIR/sub/a%20b.html
"sub/a%20b.html"	url	true	1|file://DIR/sub/a%20b.html
"sub/a%20b.html"	resolved	true	1|file://DIR/sub/a%20b.html
"../up.txt"	string	true	1|qrc:/qs-blackhole
"../up.txt"	url	true	1|qrc:/qs-blackhole
"../up.txt"	resolved	true	1|qrc:/qs-blackhole
"./x.txt?q=a%20b&r=c d#f g"	string	true	1|file://DIR/x.txt?q=a%20b&r=c%20d#f%20g
"./x.txt?q=a%20b&r=c d#f g"	url	true	1|file://DIR/x.txt?q=a%20b&r=c%20d#f%20g
"./x.txt?q=a%20b&r=c d#f g"	resolved	true	1|file://DIR/x.txt?q=a%20b&r=c%20d#f%20g
"sub/../a.txt"	string	true	1|file://DIR/a.txt
"sub/../a.txt"	url	true	1|file://DIR/a.txt
"sub/../a.txt"	resolved	true	1|file://DIR/a.txt
"a b/../c d.txt"	string	true	1|file://DIR/c%20d.txt
"a b/../c d.txt"	url	true	1|file://DIR/c%20d.txt
"a b/../c d.txt"	resolved	true	1|file://DIR/c%20d.txt
"."	string	true	1|file://DIR
"."	url	true	1|file://DIR
"."	resolved	true	1|file://DIR
"./"	string	true	1|file://DIR
"./"	url	true	1|file://DIR
"./"	resolved	true	1|file://DIR
".."	string	true	1|qrc:/qs-blackhole
".."	url	true	1|qrc:/qs-blackhole
".."	resolved	true	1|qrc:/qs-blackhole
"/"	string	true	1|file:///
"/"	url	true	1|file:///
"/"	resolved	true	1|file:///
"foo/"	string	true	1|file://DIR/foo/
"foo/"	url	true	1|file://DIR/foo/
"foo/"	resolved	true	1|file://DIR/foo/
"?q=1"	string	true	1|qs:@/qs/Open.qml?q=1
"?q=1"	url	true	1|qs:@/qs/Open.qml?q=1
"?q=1"	resolved	true	1|qs:@/qs/Open.qml?q=1
"?q=ü x"	string	true	1|qs:@/qs/Open.qml?q=%C3%BC%20x
"?q=ü x"	url	true	1|qs:@/qs/Open.qml?q=%C3%BC%20x
"?q=ü x"	resolved	true	1|qs:@/qs/Open.qml?q=%C3%BC%20x
"#frag"	string	true	1|qs:@/qs/Open.qml#frag
"#frag"	url	true	1|qs:@/qs/Open.qml#frag
"#frag"	resolved	true	1|qs:@/qs/Open.qml#frag
"#ü x"	string	true	1|qs:@/qs/Open.qml#%C3%BC%20x
"#ü x"	url	true	1|qs:@/qs/Open.qml#%C3%BC%20x
"#ü x"	resolved	true	1|qs:@/qs/Open.qml#%C3%BC%20x
""	string	false	-
""	url	false	-
""	resolved	false	-
"a%2Fb.txt"	string	true	1|file://DIR/a/b.txt
"a%2Fb.txt"	url	true	1|file://DIR/a/b.txt
"a%2Fb.txt"	resolved	true	1|file://DIR/a/b.txt
"ünï.txt"	string	true	1|file://DIR/%C3%BCn%C3%AF.txt
"ünï.txt"	url	true	1|file://DIR/%C3%BCn%C3%AF.txt
"ünï.txt"	resolved	true	1|file://DIR/%C3%BCn%C3%AF.txt
"%C3%BC.txt"	string	true	1|file://DIR/%C3%BC.txt
"%C3%BC.txt"	url	true	1|file://DIR/%C3%BC.txt
"%C3%BC.txt"	resolved	true	1|file://DIR/%C3%BC.txt
"a%E4b.txt"	string	true	1|file://DIR/a%EF%BF%BDb.txt
"a%E4b.txt"	url	true	1|file://DIR/a%EF%BF%BDb.txt
"a%E4b.txt"	resolved	true	1|file://DIR/a%EF%BF%BDb.txt
"a+b.txt"	string	true	1|file://DIR/a+b.txt
"a+b.txt"	url	true	1|file://DIR/a+b.txt
"a+b.txt"	resolved	true	1|file://DIR/a+b.txt
"a%25b.txt"	string	true	1|file://DIR/a%25b.txt
"a%25b.txt"	url	true	1|file://DIR/a%25b.txt
"a%25b.txt"	resolved	true	1|file://DIR/a%25b.txt
"a%zzb.txt"	string	true	1|file://DIR/a%25zzb.txt
"a%zzb.txt"	url	true	1|file://DIR/a%25zzb.txt
"a%zzb.txt"	resolved	true	1|file://DIR/a%25zzb.txt
"rel with \"quote\".txt"	string	true	1|file://DIR/rel%20with%20%22quote%22.txt
"rel with \"quote\".txt"	url	true	1|file://DIR/rel%20with%20%22quote%22.txt
"rel with \"quote\".txt"	resolved	true	1|file://DIR/rel%20with%20%22quote%22.txt
"a|b.txt"	string	true	1|file://DIR/a%7Cb.txt
"a|b.txt"	url	true	1|file://DIR/a%7Cb.txt
"a|b.txt"	resolved	true	1|file://DIR/a%7Cb.txt
"a\\b.txt"	string	true	1|file://DIR/a%5Cb.txt
"a\\b.txt"	url	true	1|file://DIR/a%5Cb.txt
"a\\b.txt"	resolved	true	1|file://DIR/a%5Cb.txt
"a<b>c.txt"	string	true	1|file://DIR/a%3Cb%3Ec.txt
"a<b>c.txt"	url	true	1|file://DIR/a%3Cb%3Ec.txt
"a<b>c.txt"	resolved	true	1|file://DIR/a%3Cb%3Ec.txt
"a{b}c^d`e.txt"	string	true	1|file://DIR/a%7Bb%7Dc%5Ed%60e.txt
"a{b}c^d`e.txt"	url	true	1|file://DIR/a%7Bb%7Dc%5Ed%60e.txt
"a{b}c^d`e.txt"	resolved	true	1|file://DIR/a%7Bb%7Dc%5Ed%60e.txt
"a[b]c.txt"	string	true	1|file://DIR/a%5Bb%5Dc.txt
"a[b]c.txt"	url	true	1|file://DIR/a%5Bb%5Dc.txt
"a[b]c.txt"	resolved	true	1|file://DIR/a%5Bb%5Dc.txt
"a'b(c)!*.txt"	string	true	1|file://DIR/a'b(c)!*.txt
"a'b(c)!*.txt"	url	true	1|file://DIR/a'b(c)!*.txt
"a'b(c)!*.txt"	resolved	true	1|file://DIR/a'b(c)!*.txt
" spaced.txt "	string	true	1|file://DIR/%20spaced.txt%20
" spaced.txt "	url	true	1|file://DIR/%20spaced.txt%20
" spaced.txt "	resolved	true	1|file://DIR/%20spaced.txt%20
"\ttab.txt"	string	true	1|file://DIR/%09tab.txt
"\ttab.txt"	url	true	1|file://DIR/%09tab.txt
"\ttab.txt"	resolved	true	1|file://DIR/%09tab.txt
"a\nb.txt"	string	true	1|file://DIR/a%0Ab.txt
"a\nb.txt"	url	true	1|file://DIR/a%0Ab.txt
"a\nb.txt"	resolved	true	1|file://DIR/a%0Ab.txt
"~/.config/x"	string	true	1|file://DIR/~/.config/x
"~/.config/x"	url	true	1|file://DIR/~/.config/x
"~/.config/x"	resolved	true	1|file://DIR/~/.config/x
"$HOME/x y"	string	true	1|file://DIR/$HOME/x%20y
"$HOME/x y"	url	true	1|file://DIR/$HOME/x%20y
"$HOME/x y"	resolved	true	1|file://DIR/$HOME/x%20y
"www.example.org"	string	true	1|file://DIR/www.example.org
"www.example.org"	url	true	1|file://DIR/www.example.org
"www.example.org"	resolved	true	1|file://DIR/www.example.org
"www.example.org/a b"	string	true	1|file://DIR/www.example.org/a%20b
"www.example.org/a b"	url	true	1|file://DIR/www.example.org/a%20b
"www.example.org/a b"	resolved	true	1|file://DIR/www.example.org/a%20b
"/abs/a b.txt"	string	true	1|file:///abs/a%20b.txt
"/abs/a b.txt"	url	true	1|file:///abs/a%20b.txt
"/abs/a b.txt"	resolved	true	1|file:///abs/a%20b.txt
"/abs/a%20b.txt"	string	true	1|file:///abs/a%20b.txt
"/abs/a%20b.txt"	url	true	1|file:///abs/a%20b.txt
"/abs/a%20b.txt"	resolved	true	1|file:///abs/a%20b.txt
"/abs/ü?q#f"	string	true	1|file:///abs/%C3%BC?q#f
"/abs/ü?q#f"	url	true	1|file:///abs/%C3%BC?q#f
"/abs/ü?q#f"	resolved	true	1|file:///abs/%C3%BC?q#f
"file:///abs/a b.txt"	string	true	1|file:///abs/a%20b.txt
"file:///abs/a b.txt"	url	true	1|file:///abs/a%20b.txt
"file:///abs/a b.txt"	resolved	true	1|file:///abs/a%20b.txt
"file:///abs/a%20b.txt"	string	true	1|file:///abs/a%20b.txt
"file:///abs/a%20b.txt"	url	true	1|file:///abs/a%20b.txt
"file:///abs/a%20b.txt"	resolved	true	1|file:///abs/a%20b.txt
"FILE:///Abs/X y.txt"	string	true	1|file:///Abs/X%20y.txt
"FILE:///Abs/X y.txt"	url	true	1|file:///Abs/X%20y.txt
"FILE:///Abs/X y.txt"	resolved	true	1|file:///Abs/X%20y.txt
"file:rel.txt"	string	true	1|file:rel.txt
"file:rel.txt"	url	true	1|file:rel.txt
"file:rel.txt"	resolved	true	1|file:rel.txt
"file:rel dir/x.txt"	string	true	1|file:rel%20dir/x.txt
"file:rel dir/x.txt"	url	true	1|file:rel%20dir/x.txt
"file:rel dir/x.txt"	resolved	true	1|file:rel%20dir/x.txt
"//host/share/f g.txt"	string	true	1|file://host/share/f%20g.txt
"//host/share/f g.txt"	url	true	1|file://host/share/f%20g.txt
"//host/share/f g.txt"	resolved	true	1|file://host/share/f%20g.txt
"https://example.org/a b?q=c d"	string	true	1|https://example.org/a%20b?q=c%20d
"https://example.org/a b?q=c d"	url	true	1|https://example.org/a%20b?q=c%20d
"https://example.org/a b?q=c d"	resolved	true	1|https://example.org/a%20b?q=c%20d
"https://example.org/a%20b?q=c%20d#x%20y"	string	true	1|https://example.org/a%20b?q=c%20d#x%20y
"https://example.org/a%20b?q=c%20d#x%20y"	url	true	1|https://example.org/a%20b?q=c%20d#x%20y
"https://example.org/a%20b?q=c%20d#x%20y"	resolved	true	1|https://example.org/a%20b?q=c%20d#x%20y
"https://example.org/%7Euser/ü"	string	true	1|https://example.org/~user/%C3%BC
"https://example.org/%7Euser/ü"	url	true	1|https://example.org/~user/%C3%BC
"https://example.org/%7Euser/ü"	resolved	true	1|https://example.org/~user/%C3%BC
"HTTPS://Example.ORG/Path"	string	true	1|https://example.org/Path
"HTTPS://Example.ORG/Path"	url	true	1|https://example.org/Path
"HTTPS://Example.ORG/Path"	resolved	true	1|https://example.org/Path
"http://example.org:80/"	string	true	1|http://example.org:80/
"http://example.org:80/"	url	true	1|http://example.org:80/
"http://example.org:80/"	resolved	true	1|http://example.org:80/
"http://example.org:99999/"	string	false	-
"http://example.org:99999/"	url	false	-
"http://example.org:99999/"	resolved	false	-
"http://[::1]:8080/x y"	string	true	1|http://[::1]:8080/x%20y
"http://[::1]:8080/x y"	url	true	1|http://[::1]:8080/x%20y
"http://[::1]:8080/x y"	resolved	true	1|http://[::1]:8080/x%20y
"https://us er:p w@example.org/x"	string	true	1|https://us%20er:p%20w@example.org/x
"https://us er:p w@example.org/x"	url	true	1|https://us%20er:p%20w@example.org/x
"https://us er:p w@example.org/x"	resolved	true	1|https://us%20er:p%20w@example.org/x
"https://bücher.example/ü"	string	true	1|https://xn--bcher-kva.example/%C3%BC
"https://bücher.example/ü"	url	true	1|https://xn--bcher-kva.example/%C3%BC
"https://bücher.example/ü"	resolved	true	1|https://xn--bcher-kva.example/%C3%BC
"https://xn--bcher-kva.example/"	string	true	1|https://xn--bcher-kva.example/
"https://xn--bcher-kva.example/"	url	true	1|https://xn--bcher-kva.example/
"https://xn--bcher-kva.example/"	resolved	true	1|https://xn--bcher-kva.example/
"https://example.org/?q=a+b&r=%26&s=ü"	string	true	1|https://example.org/?q=a+b&r=%26&s=%C3%BC
"https://example.org/?q=a+b&r=%26&s=ü"	url	true	1|https://example.org/?q=a+b&r=%26&s=%C3%BC
"https://example.org/?q=a+b&r=%26&s=ü"	resolved	true	1|https://example.org/?q=a+b&r=%26&s=%C3%BC
"https://example.org/a/../b/./c"	string	true	1|https://example.org/a/../b/./c
"https://example.org/a/../b/./c"	url	true	1|https://example.org/a/../b/./c
"https://example.org/a/../b/./c"	resolved	true	1|https://example.org/a/../b/./c
"mailto:a@b.c?subject=x y"	string	true	1|mailto:a@b.c?subject=x%20y
"mailto:a@b.c?subject=x y"	url	true	1|mailto:a@b.c?subject=x%20y
"mailto:a@b.c?subject=x y"	resolved	true	1|mailto:a@b.c?subject=x%20y
"mailto:ü@b.c"	string	true	1|mailto:%C3%BC@b.c
"mailto:ü@b.c"	url	true	1|mailto:%C3%BC@b.c
"mailto:ü@b.c"	resolved	true	1|mailto:%C3%BC@b.c
"tel:+1 234"	string	true	1|tel:+1%20234
"tel:+1 234"	url	true	1|tel:+1%20234
"tel:+1 234"	resolved	true	1|tel:+1%20234
"magnet:?xt=urn:btih:abc&dn=a b"	string	true	1|magnet:?xt=urn:btih:abc&dn=a%20b
"magnet:?xt=urn:btih:abc&dn=a b"	url	true	1|magnet:?xt=urn:btih:abc&dn=a%20b
"magnet:?xt=urn:btih:abc&dn=a b"	resolved	true	1|magnet:?xt=urn:btih:abc&dn=a%20b
"data:text/plain,hello world"	string	true	1|data:text/plain,hello%20world
"data:text/plain,hello world"	url	true	1|data:text/plain,hello%20world
"data:text/plain,hello world"	resolved	true	1|data:text/plain,hello%20world
"about:blank"	string	true	1|about:blank
"about:blank"	url	true	1|about:blank
"about:blank"	resolved	true	1|about:blank
"javascript:alert(1)"	string	true	1|javascript:alert(1)
"javascript:alert(1)"	url	true	1|javascript:alert(1)
"javascript:alert(1)"	resolved	true	1|javascript:alert(1)
"x:y"	string	true	1|x:y
"x:y"	url	true	1|x:y
"x:y"	resolved	true	1|x:y
"a:b/c"	string	true	1|a:b/c
"a:b/c"	url	true	1|a:b/c
"a:b/c"	resolved	true	1|a:b/c
"C:/x y.txt"	string	true	1|c:/x%20y.txt
"C:/x y.txt"	url	true	1|c:/x%20y.txt
"C:/x y.txt"	resolved	true	1|c:/x%20y.txt
"localhost:8080/x"	string	true	1|localhost:8080/x
"localhost:8080/x"	url	true	1|localhost:8080/x
"localhost:8080/x"	resolved	true	1|localhost:8080/x
"qrc:/x y.qml"	string	true	1|qrc:/x%20y.qml
"qrc:/x y.qml"	url	true	1|qrc:/x%20y.qml
"qrc:/x y.qml"	resolved	true	1|qrc:/x%20y.qml

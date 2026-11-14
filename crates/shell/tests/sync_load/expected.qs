Image declared | same call | 1 sourceSize 4x3 implicit 4x3 progress 1 painted 4x3 | progress=1,painted=4x3,implicitWidth=4,implicitHeight=3,painted=4x3,status=1,sourceSize=4x3
Image with no source | same call | 0 sourceSize 0x0 implicit 0x0 progress 0 painted 0x0 | 
Image written | same call | 1 sourceSize 4x3 implicit 4x3 progress 1 painted 4x3 | source,progress=1,painted=4x3,implicitWidth=4,implicitHeight=3,painted=4x3,status=1,sourceSize=4x3
Image declared asynchronous | same call | 2 sourceSize 0x0 implicit 0x0 progress 0 painted 0x0 | status=2
Image written asynchronous | same call | 2 sourceSize 0x0 implicit 0x0 progress 0 painted 0x0 | source,status=2
Image written asynchronous, a file loaded before | same call | 1 sourceSize 4x3 implicit 4x3 progress 1 painted 4x3 | source,progress=1,painted=4x3,implicitWidth=4,implicitHeight=3,painted=4x3,status=1,sourceSize=4x3
Image declared, no such file | same call | 3 sourceSize 0x0 implicit 0x0 progress 0 painted 0x0 | painted=0x0,status=3,sourceSize=0x0
Image written, no such file | same call | 3 sourceSize 0x0 implicit 0x0 progress 0 painted 0x0 | source,painted=0x0,status=3,sourceSize=0x0
Image written asynchronous, no such file | same call | 2 sourceSize 0x0 implicit 0x0 progress 0 painted 0x0 | source,status=2
Image written, not a picture | same call | 3 sourceSize 0x0 implicit 0x0 progress 0 painted 0x0 | source,painted=0x0,status=3,sourceSize=0x0
Image written, a url value | same call | 1 sourceSize 4x3 implicit 4x3 progress 1 painted 4x3 | source,progress=1,painted=4x3,implicitWidth=4,implicitHeight=3,painted=4x3,status=1,sourceSize=4x3
Image written, sourceSize 2x2 | same call | 1 sourceSize 2x2 implicit 2x2 progress 1 painted 2x2 | source,progress=1,painted=2x2,implicitWidth=2,implicitHeight=2,painted=2x2,status=1,sourceSize=2x2
Image written, sourceSize.width 8 | same call | 1 sourceSize 8x0 implicit 4x3 progress 1 painted 4x3 | source,progress=1,painted=4x3,implicitWidth=4,implicitHeight=3,painted=4x3,status=1,sourceSize=8x0
Image written over a loaded one | same call | 1 sourceSize 4x3 implicit 4x3 progress 1 painted 4x3 | source,painted=4x3,status=1
Image written empty over a loaded one | same call | 0 sourceSize 0x0 implicit 0x0 progress 0 painted 0x0 | source,progress=0,painted=0x0,implicitWidth=0,implicitHeight=0,painted=0x0,status=0,sourceSize=0x0
Image written over a loading one | same call | 2 sourceSize 0x0 implicit 0x0 progress 0 painted 0x0 | source,status=2
Image written, not visible | same call | 1 sourceSize 4x3 implicit 4x3 progress 1 painted 4x3 | source,progress=1,painted=4x3,implicitWidth=4,implicitHeight=3,painted=4x3,status=1,sourceSize=4x3
Image made asynchronous after it loaded | same call | 1 sourceSize 4x3 implicit 4x3 progress 1 painted 4x3 | 
Image sourceSize written after it loaded | same call | 1 sourceSize 2x2 implicit 2x2 progress 1 painted 2x2 | sourceSize=2x2,painted=2x2,implicitWidth=2,implicitHeight=2,painted=2x2,status=1,sourceSize=2x2
Image of the document, in its Component.onCompleted | same call | 1 sourceSize 4x3 implicit 4x3 progress 1 painted 4x3 | 
Image by createObject(root, {source}) | same call | 1 sourceSize 4x3 implicit 4x3 progress 1 painted 4x3 | status=1
Image by createObject(root, {source, asynchronous}) | same call | 2 sourceSize 0x0 implicit 0x0 progress 0 painted 0x0 | status=2
Image by createObject(root, {asynchronous, source}) | same call | 2 sourceSize 0x0 implicit 0x0 progress 0 painted 0x0 | status=2
Image written, cache: false | same call | 1 sourceSize 4x3 implicit 4x3 progress 1 painted 4x3 | source,progress=1,painted=4x3,implicitWidth=4,implicitHeight=3,painted=4x3,status=1,sourceSize=4x3
Image written, http | same call | 2 sourceSize 0x0 implicit 0x0 progress 0 painted 0x0 | source,status=2
BorderImage declared | same call | 1 sourceSize 4x3 implicit 4x3 progress 1 | implicitWidth=4,implicitHeight=3,status=1,progress=1,sourceSize=4x3
BorderImage with no source | same call | 0 sourceSize 0x0 implicit 0x0 progress 0 | 
BorderImage written | same call | 1 sourceSize 4x3 implicit 4x3 progress 1 | source,implicitWidth=4,implicitHeight=3,status=1,progress=1,sourceSize=4x3
BorderImage declared asynchronous | same call | 2 sourceSize 0x0 implicit 0x0 progress 0 | status=2
BorderImage written asynchronous | same call | 2 sourceSize 0x0 implicit 0x0 progress 0 | source,status=2
BorderImage written asynchronous, a file loaded before | same call | 1 sourceSize 4x3 implicit 4x3 progress 1 | source,implicitWidth=4,implicitHeight=3,status=1,progress=1,sourceSize=4x3
BorderImage declared, no such file | same call | 3 sourceSize 0x0 implicit -1x-1 progress 0 | implicitWidth=-1,implicitHeight=-1,status=3,sourceSize=0x0
BorderImage written, no such file | same call | 3 sourceSize 0x0 implicit -1x-1 progress 0 | source,implicitWidth=-1,implicitHeight=-1,status=3,sourceSize=0x0
BorderImage written asynchronous, no such file | same call | 2 sourceSize 0x0 implicit 0x0 progress 0 | source,status=2
BorderImage written, not a picture | same call | 3 sourceSize 0x0 implicit -1x-1 progress 0 | source,implicitWidth=-1,implicitHeight=-1,status=3,sourceSize=0x0
BorderImage written, a url value | same call | 1 sourceSize 4x3 implicit 4x3 progress 1 | source,implicitWidth=4,implicitHeight=3,status=1,progress=1,sourceSize=4x3
BorderImage written over a loaded one | same call | 1 sourceSize 4x3 implicit 4x3 progress 1 | source,status=1
BorderImage written empty over a loaded one | same call | 0 sourceSize 0x0 implicit 0x0 progress 0 | source,progress=0,implicitWidth=0,implicitHeight=0,status=0,sourceSize=0x0
BorderImage written over a loading one | same call | 2 sourceSize 0x0 implicit 0x0 progress 0 | source,status=2
BorderImage written, not visible | same call | 1 sourceSize 4x3 implicit 4x3 progress 1 | source,implicitWidth=4,implicitHeight=3,status=1,progress=1,sourceSize=4x3
BorderImage made asynchronous after it loaded | same call | 1 sourceSize 4x3 implicit 4x3 progress 1 | 
AnimatedImage declared | same call | 1 sourceSize 40x30 implicit 40x30 progress 1 painted 40x30 | progress=1,painted=40x30,implicitWidth=40,implicitHeight=30,painted=40x30,painted=40x30,sourceSize=40x30,status=1
AnimatedImage with no source | same call | 0 sourceSize 0x0 implicit 0x0 progress 0 painted 0x0 | painted=0x0,sourceSize=0x0
AnimatedImage written | same call | 1 sourceSize 40x30 implicit 40x30 progress 1 painted 40x30 | painted=0x0,source,progress=1,painted=40x30,implicitWidth=40,implicitHeight=30,painted=40x30,painted=40x30,sourceSize=40x30,status=1
AnimatedImage declared asynchronous | same call | 1 sourceSize 40x30 implicit 40x30 progress 1 painted 40x30 | progress=1,painted=40x30,implicitWidth=40,implicitHeight=30,painted=40x30,painted=40x30,sourceSize=40x30,status=1
AnimatedImage written asynchronous | same call | 1 sourceSize 40x30 implicit 40x30 progress 1 painted 40x30 | painted=0x0,source,progress=1,painted=40x30,implicitWidth=40,implicitHeight=30,painted=40x30,painted=40x30,sourceSize=40x30,status=1
AnimatedImage written asynchronous, a file loaded before | same call | 1 sourceSize 40x30 implicit 40x30 progress 1 painted 40x30 | painted=0x0,source,progress=1,painted=40x30,implicitWidth=40,implicitHeight=30,painted=40x30,painted=40x30,sourceSize=40x30,status=1
AnimatedImage declared, no such file | same call | 3 sourceSize 0x0 implicit 0x0 progress 0 painted 0x0 | painted=0x0,sourceSize=0x0,status=3
AnimatedImage written, no such file | same call | 3 sourceSize 0x0 implicit 0x0 progress 0 painted 0x0 | painted=0x0,source,painted=0x0,status=3
AnimatedImage written asynchronous, no such file | same call | 3 sourceSize 0x0 implicit 0x0 progress 0 painted 0x0 | painted=0x0,source,painted=0x0,status=3
AnimatedImage written, not a picture | same call | 3 sourceSize 0x0 implicit 0x0 progress 0 painted 0x0 | painted=0x0,source,painted=0x0,status=3
AnimatedImage written, a url value | same call | 1 sourceSize 40x30 implicit 40x30 progress 1 painted 40x30 | painted=0x0,source,progress=1,painted=40x30,implicitWidth=40,implicitHeight=30,painted=40x30,painted=40x30,sourceSize=40x30,status=1
AnimatedImage written, sourceSize 2x2 | same call | 1 sourceSize 2x2 implicit 2x2 progress 1 painted 2x2 | painted=0x0,source,progress=1,painted=2x2,implicitWidth=2,implicitHeight=2,painted=2x2,painted=2x2,status=1
AnimatedImage written, sourceSize.width 8 | same call | 1 sourceSize 8x0 implicit 8x6 progress 1 painted 8x6 | painted=0x0,source,progress=1,painted=8x6,implicitWidth=8,implicitHeight=6,painted=8x6,painted=8x6,status=1
AnimatedImage written over a loaded one | same call | 1 sourceSize 40x30 implicit 40x30 progress 1 painted 40x30 | painted=0x0,implicitWidth=0,implicitHeight=0,painted=0x0,source,painted=40x30,implicitWidth=40,implicitHeight=30,painted=40x30,painted=40x30,status=1
AnimatedImage written empty over a loaded one | same call | 0 sourceSize 0x0 implicit 0x0 progress 0 painted 0x0 | painted=0x0,implicitWidth=0,implicitHeight=0,painted=0x0,source,progress=0,painted=0x0,sourceSize=0x0,status=0
AnimatedImage written over a loading one | same call | 1 sourceSize 40x30 implicit 40x30 progress 1 painted 40x30 | painted=0x0,implicitWidth=0,implicitHeight=0,painted=0x0,source,painted=40x30,implicitWidth=40,implicitHeight=30,painted=40x30,painted=40x30,status=1
AnimatedImage written, not visible | same call | 1 sourceSize 40x30 implicit 40x30 progress 1 painted 40x30 | painted=0x0,source,progress=1,painted=40x30,implicitWidth=40,implicitHeight=30,painted=40x30,painted=40x30,sourceSize=40x30,status=1
AnimatedImage made asynchronous after it loaded | same call | 1 sourceSize 40x30 implicit 40x30 progress 1 painted 40x30 | 
AnimatedImage sourceSize written after it loaded | same call | 1 sourceSize 2x2 implicit 2x2 progress 1 painted 2x2 | sourceSize=2x2,painted=2x2,implicitWidth=2,implicitHeight=2,painted=2x2,painted=2x2,sourceSize=2x2,status=1
Loader declared | same call | 1 item leaf progress 1 | completed tag=leaf status=0 item=false,item=leaf,source,status=1,progress=1,loaded
Loader written | same call | 1 item leaf progress 1 | completed tag=leaf status=0 item=false,item=leaf,source,status=1,progress=1,loaded
Loader written again, the same | same call | 1 item leaf progress 1 | 
Loader written again, another | same call | 1 item other progress 1 | completed tag=other,item=other,source,progress=1,loaded
Loader written empty | same call | 0 item null progress 0 | source,status=0,progress=0,item=null
Loader declared asynchronous | same call | 2 item null progress 0 | status=2,progress=0,source,item=null
Loader written asynchronous | same call | 2 item null progress 0 | status=2,progress=0,source,item=null
Loader declared, no such file | same call | 3 item null progress 1 | source,status=3,progress=1,item=null
Loader written, no such file | same call | 3 item null progress 1 | source,status=3,progress=1,item=null
Loader written, a file with an error | same call | 3 item null progress 1 | source,status=3,progress=1,item=null
Loader not active | same call | 0 item null progress 0 | 
Loader made active | same call | 1 item leaf progress 1 | completed tag=leaf status=0 item=false,item=leaf,source,status=1,progress=1,loaded,active=true
Loader made inactive | same call | 0 item null progress 1 | item=null,status=0,active=false
Loader sourceComponent | same call | 1 item inline progress 1 | item=inline,sourceComponent,status=1,progress=1,loaded
Loader written, a url value | same call | 1 item leaf progress 1 | completed tag=leaf status=0 item=false,item=leaf,source,status=1,progress=1,loaded
setSource(url) | same call | 1 item leaf progress 1 | completed tag=leaf status=0 item=false,item=leaf,source,status=1,progress=1,loaded
setSource(url) again, the same | same call | 1 item leaf progress 1 | completed tag=leaf status=1 item=false,item=leaf,source,progress=1,loaded
setSource(url, {tag}) over it | same call | 1 item given progress 1 | tag=given,completed tag=given status=1 item=false,item=given,source,progress=1,loaded
setSource(url, {tag}) again, the same | same call | 1 item given progress 1 | tag=given,completed tag=given status=1 item=false,item=given,source,progress=1,loaded
setSource(url, {tag}) again, another tag | same call | 1 item second progress 1 | tag=second,completed tag=second status=1 item=false,item=second,source,progress=1,loaded
setSource(another url) | same call | 1 item other progress 1 | completed tag=other,item=other,source,progress=1,loaded
source read after setSource | same call | 1 item other progress 1 |  | returns DIR/Other.qml
setSource("") | same call | 0 item null progress 0 | source,status=0,progress=0,item=null
setSource(url value, {tag}) | same call | 1 item by url progress 1 | tag=by url,completed tag=by url status=0 item=false,item=by url,source,status=1,progress=1,loaded
setSource(url, {tag}) asynchronous | same call | 2 item null progress 0 | status=2,progress=0,source,item=null
setSource, no such file | same call | 3 item null progress 1 | source,status=3,progress=1,item=null
setSource, a file with an error | same call | 3 item null progress 1 | source,status=3,progress=1,item=null
setSource() | same call | 0 item null progress 0 |  | throws Error: Unable to determine callable overload.  Candidates are: |     setSource(QUrl) |     setSource(QUrl,QJSValue)
setSource(1) | same call | 0 item null progress 0 |  | throws TypeError: Passing incompatible arguments to C++ functions from JavaScript is not allowed.
setSource(null) | same call | 0 item null progress 0 |  | throws TypeError: Passing incompatible arguments to C++ functions from JavaScript is not allowed.
setSource(undefined) | same call | 0 item null progress 0 |  | throws TypeError: Passing incompatible arguments to C++ functions from JavaScript is not allowed.
setSource(url, 1) | same call | 0 item null progress 0 | 
setSource(url, null) | same call | 0 item null progress 0 | 
setSource(url, {}, 3) | same call | 1 item leaf progress 1 | completed tag=leaf status=0 item=false,item=leaf,source,status=1,progress=1,loaded
setSource(url, {unknown}) | same call | 1 item leaf progress 1 | completed tag=leaf status=0 item=false,item=leaf,source,status=1,progress=1,loaded
setSource(url, {tag}) while not active | same call | 0 item null progress 0 | source
made active after setSource | same call | 1 item kept progress 1 | tag=kept,completed tag=kept status=0 item=false,item=kept,source,status=1,progress=1,loaded,active=true
setSource(url, {tag}) over a declared source | same call | 1 item over progress 1 | tag=over,completed tag=over status=1 item=false,item=over,source,progress=1,loaded
source written over setSource | same call | 1 item leaf progress 1 | completed tag=leaf status=1 item=false,item=leaf,source,progress=1,loaded
createComponent(url) | same call | 1 progress 1  | 
createComponent(url, Asynchronous) | same call | 2 progress 0  | 
createComponent(a file not loaded before, Asynchronous) | same call | 2 progress 0  | 
createComponent(no such file) | same call | 3 progress 1 DIR/None4.qml: File not found | 
createComponent(no such file, Asynchronous) | same call | 2 progress 0  | 
createComponent(a file with an error) | same call | 3 progress 1 DIR/parse_errors/Bad.qml:4:1: Expected token `;' | 
createComponent(url value) | same call | 1 progress 1  | 
FontLoader declared | same call | 1 name "Noto Sans Gothic" | 
FontLoader written | same call | 1 name "Noto Sans Gothic" | source,name=Noto Sans Gothic,status=1
FontLoader written again, the same | same call | 1 name "Noto Sans Gothic" | 
FontLoader declared, no such file | same call | 3 name "" | 
FontLoader written, no such file | same call | 3 name "" | source,status=3
FontLoader written, not a font | same call | 3 name "" | source,status=3
FontLoader written, a url value | same call | 1 name "Noto Sans Gothic" | source,name=Noto Sans Gothic,status=1
FileView declared, blockLoading: false; preload: true | same call | loaded false | path
FileView declared, blockLoading: false; preload: true, text() | same call | loaded false |  | returns ""
FileView written, blockLoading: false; preload: true | same call | loaded false | path
FileView written, blockLoading: false; preload: true, text() | same call | loaded false |  | returns ""
FileView written, blockLoading: false; preload: true, text() again | same call | loaded false |  | returns ""
FileView written, no such file, blockLoading: false; preload: true | same call | loaded false | path
FileView written, no such file, blockLoading: false; preload: true, text() | same call | loaded false |  | returns ""
FileView written, blockLoading: false; preload: true, waitForJob() | same call | loaded true | path,loaded=true,text,loaded | returns true
FileView written, blockLoading: false; preload: true, text() after waitForJob() | same call | loaded true |  | returns "the note\n"
FileView declared, blockLoading: false; preload: true, reload() | same call | loaded false | 
FileView declared, blockLoading: false; preload: true, text() after reload() | same call | loaded false |  | returns ""
FileView declared, blockLoading: false; preload: false | same call | loaded false | text,path
FileView declared, blockLoading: false; preload: false, text() | same call | loaded false |  | returns ""
FileView written, blockLoading: false; preload: false | same call | loaded false | text,path
FileView written, blockLoading: false; preload: false, text() | same call | loaded false |  | returns ""
FileView written, blockLoading: false; preload: false, text() again | same call | loaded false |  | returns ""
FileView written, no such file, blockLoading: false; preload: false | same call | loaded false | text,path
FileView written, no such file, blockLoading: false; preload: false, text() | same call | loaded false |  | returns ""
FileView written, blockLoading: false; preload: false, waitForJob() | same call | loaded false | text,path | returns false
FileView written, blockLoading: false; preload: false, text() after waitForJob() | same call | loaded false |  | returns ""
FileView declared, blockLoading: false; preload: false, reload() | same call | loaded false | text
FileView declared, blockLoading: false; preload: false, text() after reload() | same call | loaded false |  | returns ""
FileView declared, blockLoading: true; preload: true | same call | loaded false | path
FileView declared, blockLoading: true; preload: true, text() | same call | loaded true | loaded=true,text,loaded | returns "the note\n"
FileView written, blockLoading: true; preload: true | same call | loaded false | path
FileView written, blockLoading: true; preload: true, text() | same call | loaded true | loaded=true,text,loaded | returns "the note\n"
FileView written, blockLoading: true; preload: true, text() again | same call | loaded true |  | returns "the note\n"
FileView written, no such file, blockLoading: true; preload: true | same call | loaded false | path
FileView written, no such file, blockLoading: true; preload: true, text() | same call | loaded false | text,loadFailed=2 | returns ""
FileView written, blockLoading: true; preload: true, waitForJob() | same call | loaded true | path,loaded=true,text,loaded | returns true
FileView written, blockLoading: true; preload: true, text() after waitForJob() | same call | loaded true |  | returns "the note\n"
FileView declared, blockLoading: true; preload: true, reload() | same call | loaded false | 
FileView declared, blockLoading: true; preload: true, text() after reload() | same call | loaded true | loaded=true,text,loaded | returns "the note\n"
FileView declared, blockLoading: true; preload: false | same call | loaded false | text,path
FileView declared, blockLoading: true; preload: false, text() | same call | loaded true | loaded=true,text,loaded | returns "the note\n"
FileView written, blockLoading: true; preload: false | same call | loaded false | text,path
FileView written, blockLoading: true; preload: false, text() | same call | loaded true | loaded=true,text,loaded | returns "the note\n"
FileView written, blockLoading: true; preload: false, text() again | same call | loaded true |  | returns "the note\n"
FileView written, no such file, blockLoading: true; preload: false | same call | loaded false | text,path
FileView written, no such file, blockLoading: true; preload: false, text() | same call | loaded false | text,loadFailed=2 | returns ""
FileView written, blockLoading: true; preload: false, waitForJob() | same call | loaded false | text,path | returns false
FileView written, blockLoading: true; preload: false, text() after waitForJob() | same call | loaded true | loaded=true,text,loaded | returns "the note\n"
FileView declared, blockLoading: true; preload: false, reload() | same call | loaded false | text
FileView declared, blockLoading: true; preload: false, text() after reload() | same call | loaded true | loaded=true,text,loaded | returns "the note\n"
FileView written, blockAllReads: true | same call | loaded false | path
FileView written, blockAllReads: true, text() | same call | loaded true | loaded=true,text,loaded | returns "the note\n"
FileView written, a string path | same call | loaded false | path
IconImage declared | same call | 1 implicit 16x16 backer 16x16 21x16 | source,implicitWidth=16,status=1
IconImage with no source | same call | 0 implicit 16x16 backer 16x16 0x0 | implicitWidth=16
IconImage written | same call | 1 implicit 16x16 backer 16x16 21x16 | source,status=1
IconImage declared asynchronous | same call | 2 implicit 16x16 backer 16x16 0x0 | source,implicitWidth=16,status=2
IconImage written, 8x8 | same call | 1 implicit 0x0 backer 8x8 11x8 | source,status=1
IconImage written, no such file | same call | 3 implicit 16x16 backer 16x16 0x0 | source,status=3
Image declared | later | 1 sourceSize 4x3 implicit 4x3 progress 1 painted 4x3 | 
Image written | later | 1 sourceSize 4x3 implicit 4x3 progress 1 painted 4x3 | 
Image declared asynchronous | later | 1 sourceSize 4x3 implicit 4x3 progress 1 painted 4x3 | progress=1,painted=4x3,implicitWidth=4,implicitHeight=3,painted=4x3,status=1,sourceSize=4x3
Image written asynchronous | later | 1 sourceSize 4x3 implicit 4x3 progress 1 painted 4x3 | progress=1,painted=4x3,implicitWidth=4,implicitHeight=3,painted=4x3,status=1,sourceSize=4x3
Image written asynchronous, a file loaded before | later | 1 sourceSize 4x3 implicit 4x3 progress 1 painted 4x3 | 
Image declared, no such file | later | 3 sourceSize 0x0 implicit 0x0 progress 0 painted 0x0 | 
Image written, no such file | later | 3 sourceSize 0x0 implicit 0x0 progress 0 painted 0x0 | 
Image written asynchronous, no such file | later | 3 sourceSize 0x0 implicit 0x0 progress 0 painted 0x0 | painted=0x0,status=3,sourceSize=0x0
Image written, not a picture | later | 3 sourceSize 0x0 implicit 0x0 progress 0 painted 0x0 | 
Image written, a url value | later | 1 sourceSize 4x3 implicit 4x3 progress 1 painted 4x3 | 
Image written, sourceSize 2x2 | later | 1 sourceSize 2x2 implicit 2x2 progress 1 painted 2x2 | 
Image written, sourceSize.width 8 | later | 1 sourceSize 8x0 implicit 4x3 progress 1 painted 4x3 | 
Image written empty over a loaded one | later | 0 sourceSize 0x0 implicit 0x0 progress 0 painted 0x0 | 
Image written over a loading one | later | 1 sourceSize 4x3 implicit 4x3 progress 1 painted 4x3 | progress=1,painted=4x3,implicitWidth=4,implicitHeight=3,painted=4x3,status=1,sourceSize=4x3
Image written, not visible | later | 1 sourceSize 4x3 implicit 4x3 progress 1 painted 4x3 | 
Image made asynchronous after it loaded | later | 1 sourceSize 4x3 implicit 4x3 progress 1 painted 4x3 | 
Image sourceSize written after it loaded | later | 1 sourceSize 2x2 implicit 2x2 progress 1 painted 2x2 | 
Image of the document, in its Component.onCompleted | later | 1 sourceSize 4x3 implicit 4x3 progress 1 painted 4x3 | 
Image by createObject(root, {source}) | later | 1 sourceSize 4x3 implicit 4x3 progress 1 painted 4x3 | 
Image by createObject(root, {source, asynchronous}) | later | 1 sourceSize 4x3 implicit 4x3 progress 1 painted 4x3 | status=1
Image by createObject(root, {asynchronous, source}) | later | 1 sourceSize 4x3 implicit 4x3 progress 1 painted 4x3 | status=1
Image written, cache: false | later | 1 sourceSize 4x3 implicit 4x3 progress 1 painted 4x3 | 
Image written, http | later | 3 sourceSize 0x0 implicit 0x0 progress 0 painted 0x0 | painted=0x0,status=3,sourceSize=0x0
BorderImage declared | later | 1 sourceSize 4x3 implicit 4x3 progress 1 | 
BorderImage written | later | 1 sourceSize 4x3 implicit 4x3 progress 1 | 
BorderImage declared asynchronous | later | 1 sourceSize 4x3 implicit 4x3 progress 1 | implicitWidth=4,implicitHeight=3,status=1,progress=1,sourceSize=4x3
BorderImage written asynchronous | later | 1 sourceSize 4x3 implicit 4x3 progress 1 | implicitWidth=4,implicitHeight=3,status=1,progress=1,sourceSize=4x3
BorderImage written asynchronous, a file loaded before | later | 1 sourceSize 4x3 implicit 4x3 progress 1 | 
BorderImage declared, no such file | later | 3 sourceSize 0x0 implicit -1x-1 progress 0 | 
BorderImage written, no such file | later | 3 sourceSize 0x0 implicit -1x-1 progress 0 | 
BorderImage written asynchronous, no such file | later | 3 sourceSize 0x0 implicit -1x-1 progress 0 | implicitWidth=-1,implicitHeight=-1,status=3,sourceSize=0x0
BorderImage written, not a picture | later | 3 sourceSize 0x0 implicit -1x-1 progress 0 | 
BorderImage written, a url value | later | 1 sourceSize 4x3 implicit 4x3 progress 1 | 
BorderImage written empty over a loaded one | later | 0 sourceSize 0x0 implicit 0x0 progress 0 | 
BorderImage written over a loading one | later | 1 sourceSize 4x3 implicit 4x3 progress 1 | implicitWidth=4,implicitHeight=3,status=1,progress=1,sourceSize=4x3
BorderImage written, not visible | later | 1 sourceSize 4x3 implicit 4x3 progress 1 | 
BorderImage made asynchronous after it loaded | later | 1 sourceSize 4x3 implicit 4x3 progress 1 | 
AnimatedImage declared | later | 1 sourceSize 40x30 implicit 40x30 progress 1 painted 40x30 | 
AnimatedImage written | later | 1 sourceSize 40x30 implicit 40x30 progress 1 painted 40x30 | 
AnimatedImage declared asynchronous | later | 1 sourceSize 40x30 implicit 40x30 progress 1 painted 40x30 | 
AnimatedImage written asynchronous | later | 1 sourceSize 40x30 implicit 40x30 progress 1 painted 40x30 | 
AnimatedImage written asynchronous, a file loaded before | later | 1 sourceSize 40x30 implicit 40x30 progress 1 painted 40x30 | 
AnimatedImage declared, no such file | later | 3 sourceSize 0x0 implicit 0x0 progress 0 painted 0x0 | 
AnimatedImage written, no such file | later | 3 sourceSize 0x0 implicit 0x0 progress 0 painted 0x0 | 
AnimatedImage written asynchronous, no such file | later | 3 sourceSize 0x0 implicit 0x0 progress 0 painted 0x0 | 
AnimatedImage written, not a picture | later | 3 sourceSize 0x0 implicit 0x0 progress 0 painted 0x0 | 
AnimatedImage written, a url value | later | 1 sourceSize 40x30 implicit 40x30 progress 1 painted 40x30 | 
AnimatedImage written, sourceSize 2x2 | later | 1 sourceSize 2x2 implicit 2x2 progress 1 painted 2x2 | 
AnimatedImage written, sourceSize.width 8 | later | 1 sourceSize 8x0 implicit 8x6 progress 1 painted 8x6 | 
AnimatedImage written empty over a loaded one | later | 0 sourceSize 0x0 implicit 0x0 progress 0 painted 0x0 | 
AnimatedImage written over a loading one | later | 1 sourceSize 40x30 implicit 40x30 progress 1 painted 40x30 | 
AnimatedImage written, not visible | later | 1 sourceSize 40x30 implicit 40x30 progress 1 painted 40x30 | 
AnimatedImage made asynchronous after it loaded | later | 1 sourceSize 40x30 implicit 40x30 progress 1 painted 40x30 | 
AnimatedImage sourceSize written after it loaded | later | 1 sourceSize 2x2 implicit 2x2 progress 1 painted 2x2 | 
Loader declared | later | 1 item leaf progress 1 | 
Loader written empty | later | 0 item null progress 0 | 
Loader declared asynchronous | later | 1 item leaf progress 1 | progress=1,completed tag=leaf status=2 item=false,item=leaf,source,status=1,progress=1,loaded
Loader written asynchronous | later | 1 item leaf progress 1 | progress=1,completed tag=leaf status=2 item=false,item=leaf,source,status=1,progress=1,loaded
Loader declared, no such file | later | 3 item null progress 1 | 
Loader written, no such file | later | 3 item null progress 1 | 
Loader written, a file with an error | later | 3 item null progress 1 | 
Loader made inactive | later | 0 item null progress 1 | 
Loader sourceComponent | later | 1 item inline progress 1 | 
Loader written, a url value | later | 1 item leaf progress 1 | 
setSource("") | later | 0 item null progress 0 | 
setSource(url value, {tag}) | later | 1 item by url progress 1 | 
setSource(url, {tag}) asynchronous | later | 1 item later progress 1 | progress=1,tag=later,completed tag=later status=2 item=false,item=later,source,status=1,progress=1,loaded
setSource, no such file | later | 3 item null progress 1 | 
setSource, a file with an error | later | 3 item null progress 1 | 
setSource(url, {}, 3) | later | 1 item leaf progress 1 | 
setSource(url, {unknown}) | later | 1 item leaf progress 1 | 
made active after setSource | later | 1 item kept progress 1 | 
source written over setSource | later | 1 item leaf progress 1 | 
createComponent(url) | later | 1 progress 1  | 
createComponent(url, Asynchronous) | later | 1 progress 1  | progress=1,status=1
createComponent(a file not loaded before, Asynchronous) | later | 1 progress 1  | progress=1,status=1
createComponent(no such file) | later | 3 progress 1 DIR/None4.qml: File not found | 
createComponent(no such file, Asynchronous) | later | 3 progress 1 DIR/None5.qml: File not found | progress=1,status=3
createComponent(a file with an error) | later | 3 progress 1 DIR/parse_errors/Bad.qml:4:1: Expected token `;' | 
createComponent(url value) | later | 1 progress 1  | 
FontLoader declared | later | 1 name "Noto Sans Gothic" | 
FontLoader written again, the same | later | 1 name "Noto Sans Gothic" | 
FontLoader declared, no such file | later | 3 name "" | 
FontLoader written, no such file | later | 3 name "" | 
FontLoader written, not a font | later | 3 name "" | 
FontLoader written, a url value | later | 1 name "Noto Sans Gothic" | 
FileView declared, blockLoading: false; preload: true, text() | later | loaded true | loaded=true,text,loaded
FileView written, blockLoading: false; preload: true, text() again | later | loaded true | loaded=true,text,loaded
FileView written, no such file, blockLoading: false; preload: true, text() | later | loaded false | text,loadFailed=2
FileView written, blockLoading: false; preload: true, text() after waitForJob() | later | loaded true | 
FileView declared, blockLoading: false; preload: true, text() after reload() | later | loaded true | loaded=true,text,loaded
FileView declared, blockLoading: false; preload: false, text() | later | loaded true | loaded=true,text,loaded
FileView written, blockLoading: false; preload: false, text() again | later | loaded true | loaded=true,text,loaded
FileView written, no such file, blockLoading: false; preload: false, text() | later | loaded false | text,loadFailed=2
FileView written, blockLoading: false; preload: false, text() after waitForJob() | later | loaded true | loaded=true,text,loaded
FileView declared, blockLoading: false; preload: false, text() after reload() | later | loaded true | loaded=true,text,loaded
FileView declared, blockLoading: true; preload: true, text() | later | loaded true | 
FileView written, blockLoading: true; preload: true, text() again | later | loaded true | 
FileView written, no such file, blockLoading: true; preload: true, text() | later | loaded false | 
FileView written, blockLoading: true; preload: true, text() after waitForJob() | later | loaded true | 
FileView declared, blockLoading: true; preload: true, text() after reload() | later | loaded true | 
FileView declared, blockLoading: true; preload: false, text() | later | loaded true | 
FileView written, blockLoading: true; preload: false, text() again | later | loaded true | 
FileView written, no such file, blockLoading: true; preload: false, text() | later | loaded false | 
FileView written, blockLoading: true; preload: false, text() after waitForJob() | later | loaded true | 
FileView declared, blockLoading: true; preload: false, text() after reload() | later | loaded true | 
FileView written, blockAllReads: true, text() | later | loaded true | 
FileView written, a string path | later | loaded true | loaded=true,text,loaded
IconImage declared | later | 1 implicit 16x16 backer 16x16 21x16 | 
IconImage written | later | 1 implicit 16x16 backer 16x16 21x16 | 
IconImage declared asynchronous | later | 1 implicit 16x16 backer 16x16 21x16 | status=1
IconImage written, 8x8 | later | 1 implicit 0x0 backer 8x8 11x8 | 
IconImage written, no such file | later | 3 implicit 16x16 backer 16x16 0x0 | 

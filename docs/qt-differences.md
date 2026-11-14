# Deliberate differences from Qt

The QML engine does what Qt 6.12 does with the same QML and JavaScript. The cases below are
the ones where it does something else on purpose. Each names the test that holds the
difference: if the engine stops doing what the row says, that test fails. What rusty-impulse does
differently from Quickshell is in [quickshell-differences.md](quickshell-differences.md),
from the original in [original-differences.md](original-differences.md).

## `String.arg` of a whole number from a million that V4 holds as a double

Compared with `qml6` of Qt 6.12.0.

| | Qt 6.12 | Here |
|---|---|---|
| `"%1".arg(Math.round(1234567.8))` | `1.23457e+06` | `1234568` |
| `"%1".arg(-2147483648)` | `-2.14748e+09` | `-2147483648` |
| `"%1".arg(root.r)` with `property real r: 3000000` | `3e+06` | `3000000` |

Why: V4 stores a number as an int or as a double, and `String.arg` goes by which: an int
prints as an integer, a double as `%g` with 6 digits. The result of `Math.round`, `Math.ceil`,
`Math.trunc`, `Math.pow`, `**`, `Number()`, `parseInt`, `parseFloat`, a string turned into a
number, arithmetic with a double operand, a `real` property and the coordinates of a `point`
or `size` are doubles in V4 even when their value is whole. Numbers here have one form (an
`f64`), so a whole number within the range of an int prints as an integer. `arg` is the only
place QML code sees the difference, and only for a whole number of 1000000 or more in
magnitude: below that `%g` prints the same digits.

Everything else `arg` does with a number is the same as in Qt 6.12: a number with a
fraction, a whole number outside the range of an int (always a double in V4), int literals
and int arithmetic, `length`, `int` and `real` properties below a million.

To remove the difference a value has to carry whether it is an int or a double, as V4's
does, through every operator and built-in function, in the interpreter and in the
JavaScript compiled to Rust.

Code: `qarg_text` in `crates/qml/src/builtins.rs`.

Tests, in `crates/qml/tests/builtins_b1_qt_equiv.rs`. A `NAME.here` file beside a fixture
holds the lines this engine prints where Qt 6.12 prints another; a case in it has to print
exactly that line, and Qt's line has to differ from it.

- `string_arg_numbers` (`argnum.qml`, 263 cases): 217 print what Qt 6.12 prints, the 46 of
  `argnum.here` are whole numbers from a million that V4 holds as a double.
- `audit_fixture_2_4` (`audit_2_4.here`, case `G1`) and `connect_freeze_arg` (`misc.here`,
  cases `G1`, `G5`, `G7`): the four cases of the second engine audit.

## JavaScript newer than Qt 6.12's

V4 of Qt 6.12 lacks these; here they exist, so code that calls them gets an answer where Qt
throws `TypeError` (`Property 'flatMap' of object 1 is not a function`) or reads `undefined`:
`Symbol.matchAll`, `String.prototype.matchAll`, `replaceAll`, `at`, `trimStart` and `trimEnd`,
`Array.prototype.flat`, `flatMap` and `at`, `Object.fromEntries`, `Promise.allSettled` and
`any`, `Promise.prototype.finally`, `Symbol.prototype.description`, `RegExp.prototype.dotAll`
and `groups` of a match, `URL.canParse` and `parse`, `URLSearchParams.prototype.size`,
`Atomics.notify`, and `at`, `findLast`, `findLastIndex` and `sort` of a typed array. the original's code
uses some of them (Quickshell runs it on a newer Qt), so they stay. In the list of an
object's own keys they come after the keys Qt 6.12 has, which are in Qt's order.

Tests: `messages` (`msg.here`: `S30`, `T62`, `T110`, `T114`, `T115`, `T128`), `messages_more`
(`msg2.here`: `U64`, `U157`), `regexp_object` (`t4.here`: `T20`),
`regexp_folding_exec_template_bound` (`ecma.here`: `X9`), `own_key_order_of_every_builtin`
(`keyorder.here`, 13 objects), `not_a_function_and_error_keys` (`notfn.here`: `k20`, `k21`,
`k22`).

## An array in V4's sparse storage

An array here keeps what V4's simple storage keeps: the number of cells (`[1, , 3]` has
three, `new Array(3)` none), the attribute bytes that exist only after `freeze`, `seal` or a
`defineProperty` with other attributes than the defaults, and the byte nothing wrote, which
means "a value and no property". So a frozen array takes `a[0] = 2`, `push` and `sort`, an
element pushed onto it is read by index and is not a property for `in`, `hasOwnProperty`
and `Object.keys`, and a frozen `[1, , 3]` takes a store into its hole where a frozen
`new Array(3)` throws. An array with an accessor element takes writes as ECMA-262 says, as
V4's does. What stays different is the rest of V4's second storage, the sparse one.

| | Qt 6.12 | Here |
|---|---|---|
| `var a = [1]; a[10000] = 1; Object.freeze(a); a[0] = 2` | `TypeError`, `a[0]` is `1` | stores, `a[0]` is `2` |
| the same array, `a.push(2)` in strict code | `TypeError: Type error` | `10002` |
| `var a = []; a[5000] = 1; a[1] = 2; Object.freeze(a)` | `a[5000]` stays configurable | every element is frozen |
| `var a = [1, 2, 3]` with a getter defined at `0`, `Object.freeze(a)` or `Object.seal(a)` | the getter stays configurable, `Object.isFrozen(a)` and `isSealed(a)` are `false` | the getter is not configurable, both are `true` |
| the same after the getter is redefined as a value, `Object.freeze(a)` | element `0` stays configurable | not configurable |
| `[1, 2, 3]` with a getter that returns `7` at `1`, `push(4)`, `sort()` | `1,,3,,,,,,7,,4`, length `11` | `TypeError: Type error` (the getter has no setter) |

Why: V4 moves an array to the sparse storage when an element becomes an accessor
(`ArrayData::insert`, `qv4arraydata.cpp:589`) or when a store lands 4096 or more past the
start and a quarter past the cells there are (`qv4arraydata.cpp:569`: simple only while
`index < 0x1000 || index < size + size / 4`). The cells of the sparse storage are handed
out from a free list (`SparseArrayData::allocate`, `qv4arraydata.cpp:329`), not in index
order, and an accessor takes two. `freeze` and `seal` walk `i` over the allocated cells and
ask "is index `i` empty" while they mark the byte of cell `i` (`qv4objectproto.cpp:391` and
`417`), so in a sparse array they mark the bytes of other elements than the ones they
asked about; `sort` copies the cells out in the order of the sparse map
(`qv4arraydata.cpp:641`). The answers follow from the order in which the free list gave
the cells out. Here an array has one storage, and one that V4 would hold sparse follows
ECMA-262 in these cases; the 4096 rule is not repeated, so an array with such a gap is
still the simple one.

In `qml/` there are 0 such places: none of its `.qml`, `.js` and `.mjs` files calls
`Object.freeze`, `Object.seal`, `Object.preventExtensions`, `Object.defineProperty`,
`Object.defineProperties` or their `Reflect` forms. The test
`shell_qml_does_not_freeze_seal_or_define_properties` of
`crates/qml/tests/builtins_b1_qt_equiv.rs` holds that number: it fails on the first such
call and names this section.

Code: `Holes` and `JsObject::slots` in `crates/qml/src/value.rs`, `store_element` in
`crates/qml/src/eval.rs`, `set_integrity` in `crates/qml/src/props.rs`, `array_mutate` in
`crates/qml/src/builtins.rs`.

Tests: `array_storage_slots_and_attribute_bytes` (`arraystore.here`: `s1`, `s2`),
`array_storage_hidden_elements_and_sparse` (`arraystore2.here`: `q15`, `p1` to `p8`),
`array_sparse_storage.js` of `crates/rigen/check/cases` (the same 11 cases in JavaScript
compiled to Rust). The 97 other cases of the two fixtures print what Qt 6.12 prints.

## `this` of a derived constructor before `super(...)`

A derived constructor that has not called `super(...)` does what V4's does: `this.x = v`
runs `v` and is `TypeError: Type error` if `this` is still unbound, `delete this.x` is
`true`, a read of `this` is `ReferenceError:  is not defined`, and so is `this["m"]()`
before its key and arguments run. An arrow function of a derived constructor never has a
`this`, before `super(...)` and after it: `ReferenceError: this is not defined`. One text
is not held.

| | Qt 6.12 | Here |
|---|---|---|
| `constructor() { this.m(a) }`, after `a` runs | `Property 'm' of object 0 is not a function` from `qml6`, `... of object null is ...` from the same V4 in PySide6 6.12.0 | `Property 'm' of object 0 is not a function` |

Why: `this` holds V4's empty value there, and V4 has no text for it:
`primitiveToQString` (`qv4value.cpp:91`) marks that case unreachable, so the text is
whatever the compiler made of the neighbouring cases in that build of Qt. The fixtures
mask the word between `object` and `is`.

Code: `unbound_this` in `crates/qml/src/eval.rs` and in `crates/rigen/src/expr.rs`.

Tests: `derived_constructors` (`c9`, `c25`, `c41`), `arrow_this_and_super`,
`v4_derived_unbound_this.js` and `v4_derived_arrow.js` of `crates/rigen/check/cases`.

## `forEach` of a Map or a Set without a callback

| | Qt 6.12 | Here |
|---|---|---|
| `new Map().forEach()`, `new Set().forEach()` | `undefined` | `TypeError: Type error` |
| `new Map([[1, 2]]).forEach()`, `new Set([1]).forEach()` | `RangeError: Maximum call stack size exceeded.` | `TypeError: Type error` |

Why: V4 reads the callback `argv[0]` without a look at the number of arguments
(`qv4mapobject.cpp:274`, `qv4setobject.cpp:251`), so with none it reads the value that lies
after the arguments. Here a missing callback is refused, as a number is in both. In `qml/`
there are 0 places with `forEach()` without an argument; the test
`shell_qml_has_no_for_each_without_a_callback` of the same file holds that number.

Tests: `messages_more` (`msg2.here`: `U193`), `typed_array_base_and_collection_callbacks`
(`typedbase.here`: `m1`, `m2`).

## The address and the number in the name of a QML object

| | Qt 6.12 | Here |
|---|---|---|
| `"" + root`, the address | `QQuickItem_QML_0(0x5b28e3f9ae80)`, the pointer | `QQuickItem_QML_0(0x0)`, the number of the object in the engine |
| the number of an object after the first `Component` of its file | from source `_QML_9`, from Qt's disk cache `_QML_10` | `_QML_9` in both |
| the number of the root of a file type, `Foo_QMLTYPE_0` | changes from run to run (`_0`, `_1`, `_2` in three runs) | the order the files are first used |

Why: the address is a pointer of the process, and a test compares it by its shape (`0x` and
hexadecimal digits). The number comes from one counter of the process
(`QQmlPropertyCacheCreatorBase::classIndexCounter`, `qqmlpropertycachecreator.cpp:14`): a unit
read from the disk cache takes one more number per `Component`, and the file types take theirs
in the order the type loader finishes them, which is not fixed. The numbers here are those of
a run without the disk cache (`QML_DISABLE_DISK_CACHE=1`); within one file they equal Qt's.

Tests: `qml_object_names_define_property_type_objects` (`qobj.qml`; its Qt lines are taken
with `QML_DISABLE_DISK_CACHE=1`).

## Where an error was made

| | Qt 6.12 | Here |
|---|---|---|
| `new Error("x").lineNumber`, `.fileName` | the line and the file of the `new` | `undefined` |
| `new Error("x").stack` | `f@file.qml:43\n…`, an accessor | `""`, a data property |

Why: the engine keeps no call frames (JavaScript compiled to Rust has none), so an error is
the one V4 makes with an empty stack trace. Its own keys are V4's:
`stack,fileName,lineNumber,message`.

Tests: `not_a_function_and_error_keys` (`notfn.here`: `k7`, `k8`, `k9`, `k10`, `k16`).

## A RegExp with `i` matched more than three times

| | Qt 6.12 | Here |
|---|---|---|
| `/\u017f/i.test("S")`, six calls | `true`, `true`, `true`, `false`, `false`, `false` | `false` six times |
| `/[\u0100-\u0180]/iu.test("s")`, six calls | `false` three times, then `true` | `true` six times |

Why: V4 runs the first three matches of a RegExp in Yarr's interpreter and the later ones
in code Yarr's JIT made, and the two fold case differently for U+017F, U+1E9E, U+0131 and
U+212A. Here a RegExp answers the same every time: what the JIT answers without `u`
(ECMA-262 §22.2.2.7.3 Canonicalize), and with `u` simple case folding (§22.2.2.7.3 with
`Unicode` true).

Code: `vendor/regress/src/parse.rs`.

Tests: `regexp_folding_exec_template_bound` (`ecma.here`: `R1_9`, `R1_20`, `R2_13`, `R2_14`,
`R3_5`), `regexp_without_u` (`re.here`: `E6`).

## A reply to `XMLHttpRequest` of more than 32 MiB

Compared with Qt 6.12.0 (PySide6), a reply sent as it is, with its `Content-Length`.

| | Qt 6.12 | Here |
|---|---|---|
| A reply of 32 MiB | `status` 200, `response` holds 33554432 bytes | the same |
| A reply of 33 MiB | `status` 200, `response` holds 34603008 bytes | the request fails: `readyState` 4, `status` 0, nothing in `response`, `onerror` runs |

Why: Qt keeps whatever a server sends, so a server that never stops sending takes all the
memory of the shell. A request here keeps at most 32 MiB of a reply (`XHR_MAX` in
`crates/qml/src/http.rs`), counted after decompression, and fails when more comes. The
shell's QML asks for JSON and text of a few kilobytes; files go through the download jobs,
which write to disk and have bounds of their own.

A compressed reply has Qt's own bound as well, the same here as in Qt 6.12: one that
decodes to over 10 MiB and over 40 times (`gzip`, `deflate`) or 100 times (`br`, `zstd`)
what came ends with `status` 200 and nothing in it.

Code: `exchange` in `crates/qml/src/xhr.rs`.

Tests:

- `a_reply_over_32_mib_fails_here_and_comes_whole_in_qt` in `crates/qml/tests/xhr_equiv.rs`
  runs both rows in Qt 6.12 and here and checks both columns.
- `a_body_over_the_bound_fails_the_request` in `crates/qml/src/xhr.rs` checks that no more
  than 32 MiB is handed on, of a reply sent as it is and of a compressed one.

## A zstd frame of an old format in two chunks, the first of one byte

Compared with Qt 6.12.0 (PySide6). The reply has `Content-Encoding: zstd` and its body is
one frame of the zstd format 0.5 or 0.7 that holds `hello`.

| The body comes | Qt 6.12 | Here |
|---|---|---|
| In one piece, with its `Content-Length` | `responseText` is `hello`; states 1, 2, 3, 4, `status` 200 | the same |
| As two chunks of `Transfer-Encoding: chunked`, one byte and the rest, sent in one write | the reply is given up: states 1, 3, 4, `status` 200, `statusText` `OK`, no text, no headers, `onerror` does not run | `responseText` is `hello`; states 1, 2, 3, 4, `status` 200 |
| In two writes, one byte and 200 ms later the rest, with its `Content-Length` (measured once, on `35c8f11b`) | the reply is given up, as above | the same |

What it depends on, as measured here: the decoder is given the body in the pieces the
reader below it hands on, and a frame of an old format is read when its first piece has
five bytes of it and given up when it has one or four. The reader hands two chunks that
came together on as one piece (one read of 16 bytes for the 16 bytes of the frame 0.5), so
the frame is read. Qt gives the same reply up, in chunks and in two writes.

A frame of the format in use (1.x) whose first chunk is one byte is read in both.

Code: `ZstdFrames` in `crates/qml/src/xhr.rs`.

Tests:

- `an_old_zstd_frame_in_two_chunks_is_read_here_and_dropped_in_qt` in
  `crates/qml/tests/xhr_equiv.rs` runs the first two rows in Qt 6.12 and here and checks
  both columns.
- `chunks_that_came_together_are_read_as_one_piece` in `crates/qml/src/xhr.rs` checks the
  sizes of the reads of a body sent as the two chunks.
- `an_old_zstd_frame_is_read_when_its_first_piece_has_five_bytes` in
  `crates/qml/src/xhr.rs` gives the decoder the frames 0.5 and 0.7 whole and cut after 1, 4
  and 5 bytes. It stands for the third row: a test waits on no clock, so the cut is made
  under the decoder.

## A synchronous `XMLHttpRequest` sent from the handler of `readyState` 4

Compared with Qt 6.12.0 (PySide6). A request gets its reply, and its
`onreadystatechange` handler, called with `readyState` 4, sends the same request again
with `open(method, url, false)` and `send()`.

| | Qt 6.12 | Here |
|---|---|---|
| The handlers of the second request | run inside `send()`: `readyState` 1, 2, 3, 4 with the second reply | the same |
| After the handler that sent it returns | the process ends with a segmentation fault (signal 11) | the request stays at `readyState` 4 with the second reply; the program goes on |

Why: the fault in Qt is a defect, not behaviour a page can use, and a shell that ends
there loses everything it shows. The same request sent from `onloadend`, or sent without
`false`, does not end Qt, and is the same here as in Qt 6.12 (the cases "sent again ..."
of `responses_match_qt`).

Code: `send` in `crates/qml/src/xhr.rs`.

Test: `a_synchronous_request_from_the_handler_of_done_runs_here_and_ends_qt` in
`crates/qml/tests/xhr_equiv.rs` runs the request in Qt 6.12 and here; it fails if Qt stops
ending with signal 11, if its handlers see something else before that, or if what they see
here is not as in this table.

## A server that asks `XMLHttpRequest` for a login without end

Compared with Qt 6.12.0 (PySide6). The request has a user and a password from `open()`.
The server answers every request with 401 and
`WWW-Authenticate: Digest realm="r", nonce="n<k>", qop="auth", stale=true`, a new nonce
each time, so every answer made with the login is asked for again.

| | Qt 6.12 | Here |
|---|---|---|
| The server takes the answer to its 21st challenge | `status` 200 after 22 requests | the same |
| The server takes the answer to its 1001st challenge | `status` 200 after 1002 requests; the same from a synchronous request | the request went out 1001 times, 1000 of them with an answer; the 1001st reply is the page's: `status` 401 with its body; the same from a synchronous request |
| The server never takes an answer (Qt measured once with a probe, not in a test) | the request goes on: 82353 requests in 40 s, ended there by `abort()`, `readyState` 4, `status` 0. A synchronous request: `send()` returns after 29.7 s, states 1, 4, `status` 0, no text | `status` 401 after 1001 requests, as in the row above |

Why: Qt has no count, so a server holds a request, and with a synchronous one the whole
page for 30 s, as long as it likes. A request here answers a 401 a thousand times
(`MAX_ANSWERS` in `crates/qml/src/xhr.rs`) and then hands the page the 401. No server that
means to let the login in asks that often. What ends Qt's synchronous request is a clock
of 30 s; the count here ends it without one.

Code: `exchange` in `crates/qml/src/xhr.rs`.

Tests:

- `a_challenge_is_answered_a_thousand_times_here_and_without_end_in_qt` in
  `crates/qml/tests/xhr_equiv.rs` runs the second row in Qt 6.12 and here, with and without
  `false` in `open()`, and checks both columns.
- The cases "a nonce that is stale twenty times" and "a synchronous request with a nonce
  that is stale twenty times" of `responses_match_qt` there hold the first row.

## Six requests of `XMLHttpRequest` to one host ended by `abort()` before their replies

Compared with Qt 6.12.0 (PySide6). Qt sends the requests of an engine to one host and port
over at most six sockets. A request the server has not answered is ended with `abort()`:
`readyState` 4, `status` 0. Qt does not use the socket of such a request again, and what
that socket had (the login that goes with each request) goes with it; the same here, held
by `a_stopped_request_takes_the_login_of_its_channel_with_it` in `crates/qml/src/http.rs`.

| | Qt 6.12 | Here |
|---|---|---|
| Up to five requests ended so | the next requests go out over the sockets that are left | the same |
| Six requests ended so, one after another, each on another socket (Qt measured once with a probe) | no request of the engine to that host and port goes out any more: of two sent after them the server saw nothing and the page got no reply | the next request goes out on a new connection: its first socket, and without a login until the server asks for one |

What the server of the probe saw in Qt (seconds, connection, request, `Authorization`):
a login, the six requests it held, 200 ms apart, each ended by `abort()` before the next;
of the two requests sent after them, nothing.

```
0.02 c1 GET /c?h=Basic%20realm%3D%22r%22&ok=any&id=l HTTP/1.1 |
0.02 c2 GET /c?h=Basic%20realm%3D%22r%22&ok=any&id=l HTTP/1.1 | Authorization: Basic dTpw
0.02 c3 GET /hold?k=z1&n=99 HTTP/1.1 | Authorization: Basic dTpw
0.22 c4 GET /hold?k=z2&n=99 HTTP/1.1 |
0.42 c5 GET /hold?k=z3&n=99 HTTP/1.1 |
0.62 c6 GET /hold?k=z4&n=99 HTTP/1.1 |
0.82 c7 GET /hold?k=z5&n=99 HTTP/1.1 |
1.03 c8 GET /hold?k=z6&n=99 HTTP/1.1 |
```

Why: what Qt does there is a defect, not behaviour a page can use: a page that gave up six
slow requests can never reach that host again. Here the seventh request finds no socket
that takes requests and the connection starts anew, with six sockets that know no login,
as it does after 120 s without a request (`UNUSED` in `crates/qml/src/http.rs`).

Code: `Host::free` in `crates/qml/src/http.rs`.

Test: `six_stopped_requests_start_the_connection_anew` in `crates/qml/src/http.rs` stops six
requests, one on each socket, and checks that the seventh gets the first socket, sends no
login, and answers a challenge with the login the engine remembers. It is not run in Qt:
the requests after the six never end there, and the event loop of `xhr_equiv` runs a
page's handlers only when no request is out, so the page cannot call `abort()` on a held
request.

## The charsets of a reply to `XMLHttpRequest` that ICU reads and `encoding_rs` does not

Compared with the system's Qt 6.12.0 and its ICU 78.3 (PySide6 through
`tools/testenv/system_qt.py`). Qt reads the text of a reply with QStringDecoder: its
own decoders for the names `UTF-8`, `UTF-16`, `UTF-16LE`, `UTF-16BE`, `UTF-32`, `UTF-32LE`,
`UTF-32BE`, `ISO-8859-1`, `Latin1` and `Locale`, and for every other name the converter the
ICU it is built with opens by that name. That ICU has 228 converters under 1203 names
(it lists 1233 names; 30 of them twice, for two converters, and they open one). The list
is `crates/qml/src/xhr_charsets.tsv`, written by `crates/qml/tests/qt_icu.py names` from
ICU itself, and a name is compared as ICU compares it (its letters and digits, a number
without the zeros it starts with).

| What a reply in the charset is read as | Converters | Names |
|---|---|---|
| The text Qt reads | 47 | 399 |
| The text of a reply that names no charset: the converter is not written here | 181 | 804 |

All the names of a converter are in one row: no name of a converter that is written is
left unknown.

### The 181 converters that are not written

EBCDIC (`IBM037` and the like), the DOS code pages (`IBM437`), UTF-7, SCSU, BOCU-1,
ISO-2022-KR and -CN, HZ, LMBCS, the Indic ISCII, and the rest of ICU's tables.

| | Qt 6.12 | Here |
|---|---|---|
| `responseText` and `response` as text | the text ICU's converter makes of the bytes | the reply is read as one whose `Content-Type` names no charset: by the `encoding` of its XML declaration if it is XML, by its byte order mark or its `<meta>` if it is `text/html`, by its byte order mark, and as UTF-8 without any of these |
| The log | nothing | one line for the reply, however often and in whichever state its text is read: `XMLHttpRequest: no decoder here for the charset "IBM037", which Qt reads: the reply is read as if it named none (as UTF-8, unless its text names another)` |
| `responseXML` of a document whose XML declaration names such a charset | the document is read again with the converter | the document stays in the encoding it was read in up to there (UTF-8, or what its byte order mark says), and the log has the same line |

What that makes of the 256 bytes 0x00 to 0xFF, each once:

| Charset | Qt 6.12 | Here |
|---|---|---|
| `IBM037` (EBCDIC) | 256 characters of its table (0x25 is a line feed) | 0x00 to 0x7F as ASCII, U+FFFD for each of 0x80 to 0xFF |
| `IBM437` | 256 characters of its table | the same as for `IBM037` |
| `UTF-7` | U+FFFD for the control bytes and for 0x80 to 0xFF | the same as for `IBM037` (33 bytes differ) |
| `SCSU` | 232 characters: 0x01 to 0x1F are commands | the same as for `IBM037` |

Two of the 181 had a decoder of `encoding_rs` under their names before, and are not read
with it any more, because it is not their decoder. Each sequence of one byte and of two
was read as a reply of its own (for ISO-2022-KR also after `ESC $ ) C` and after that and
`SO`) by ICU and by that decoder:

| Charset | It was read as | Sequences | Read otherwise than by ICU |
|---|---|---|---|
| `ibm-5478` (also `chinese`, `GB_2312-80`, `iso-ir-58`, `csISO58GB231280`, `gb2312-1980`, `GB2312.1980-0`) | GBK. In ICU it has two bytes for every character and no ASCII | 65 792 | 59 213 |
| `ISO-2022-KR` (also `csISO2022KR`) | one U+FFFD for the whole reply (the "replacement" decoder of the Encoding Standard) | 197 376 | 151 142 |

Why: the QML of the shell (`qml/`) has no `XMLHttpRequest` at all, and ICU's tables for
these converters are not in the tree. A reply in one of them is not garbled in silence:
the log names the charset.

Code: `charset`, `converter` and `find` in `crates/qml/src/xhr_text.rs`; the line of the
log is `unread` there, written by `XhrRequest::say_unread` in `crates/qml/src/xhr.rs`.

Tests: `every_charset_name_of_qt_is_read_as_qt_or_is_not_written` in
`crates/qml/tests/xhr_equiv.rs` takes the names from ICU, checks that the list in the
tree is that list, and sends a reply in each of the 1203 names with each of seven bodies
(every byte once; that after each of the five byte order marks; surrogates and characters
of UTF-8 that are whole, cut and not allowed) to Qt and to the runtime. A name is in the
first row when all seven texts are Qt's, and in the second when the log has the line and
the texts are those of a name nobody knows. The test holds the numbers of the table, the count of lines in the log
(one for a reply whose text was read three times), and that no converter has names in two
rows. `ibm_5478_is_not_gbk` in `crates/qml/src/xhr_text.rs` holds that the names of
`ibm-5478` are not GBK's. `a_declaration_that_names_a_charset_not_written_here_leaves_the_text_as_it_was_read`
in `crates/qml/src/xhr_dom.rs` holds the row of `responseXML`.

### The charsets of more bytes than one: no difference, and where the characters come from

Shift_JIS, EUC-JP, Big5, Big5-HKSCS, GBK, GB2312, gb18030, EUC-KR and windows-949 are read
as ICU's table converter reads them (which bytes make a character, by the states in the
header of ICU's table; what a byte that cannot be where it is becomes; that a character
the end of the reply cuts is left out), and ISO-2022-JP as ICU's converter for it. All
ten are in the first row above. Nothing here is a difference; this says how it is kept.

The characters come from the tables of `encoding_rs`, which are those of the Encoding
Standard and not ICU's, and where ICU's table has another character, or has one and the
Standard none, or has none and the Standard one, from `crates/qml/src/xhr_charset_data.rs`.
That file is made by `tools/gen/charset/gen.py` from what the ICU of Qt reads and is not
written by hand. It has, for each charset, the bytes of a character and what ICU reads
them as (U+FFFD where ICU has no character):

| Charset (ICU's converter) | Sequences compared | Characters taken from ICU | Bytes in the program |
|---|---|---|---|
| Shift_JIS (`ibm-943_P15A-2003`) | 65 792 | 0 | 0 |
| EUC-JP (`euc-jp-2007`) | 131 584 | 24 | 180 |
| Big5 (`windows-950-2000`) | 65 792 | 6 251 | 25 004 |
| Big5-HKSCS (`ibm-1375_P100-2008`) | 65 792 | 789 | 3 156 |
| GBK (`windows-936-2000`) | 65 792 | 101 | 404 |
| GB2312 (`ibm-1383_P110-1999`) | 65 792 | 1 394 | 5 576 |
| gb18030 (`gb18030-2022`) | 1 653 392 | 0 | 0 |
| EUC-KR (`ibm-970_P110_P110-2006_U2`) | 65 792 | 190 | 760 |
| windows-949 (`windows-949-2000`) | 65 792 | 188 | 752 |
| All | | 8 937 | 35 832 |

The sequences compared are every one of one byte and of two, each read as a reply of its
own (EUC-JP also after 0x8F, gb18030 also every sequence of four bytes that can be a
character), and 60 000 streams of 3 to 12 bytes for each charset, the same on every run.

Examples of what the file has: Big5 `A4 A5` is U+F775 (a private use character; the
Standard has U+30CC); GBK `A2 E3` is U+E76C (the Standard has U+20AC); EUC-KR and
windows-949 `C9 A1` is U+E000 and EUC-JP `8F F3 A1` is U+2170 (the Standard has no
character for either).

gb18030 is the table of 2022 in ICU 78.3 (`gb18030-2022`): the byte 0x80 is U+20AC and
`A3 A0` is U+3000, as in the Standard, so nothing is taken from ICU for it. The Qt inside
the PySide6 wheel has ICU 73 with the table of 2005 (0x80 is no character, `A3 A0` is
U+E5E5): 387 of the 1 653 392 sequences are read otherwise there, and that Qt is not the
one compared with.

Code: `multi`, `Multi` and `Fixes` in `crates/qml/src/xhr_text.rs`.

Tests: `the_converters_of_icu_read_as_icu` in `crates/qml/src/xhr_text.rs` asks the ICU
that Qt loads for the same sequences and streams and holds that nothing differs for any
converter written here. `the_characters_taken_from_icu_are_in_order_and_none_is_the_tables`
there holds the count of the table and that no character of the file is one the table of
`encoding_rs` has anyway. `a_character_in_two_parts_of_a_reply_is_read_whole_as_in_qt`
in `crates/qml/tests/xhr_equiv.rs` runs Qt and the runtime on replies whose character
comes in two parts, and `gb18030_is_read_as_the_icu_of_the_system_s_qt_reads_it` there on
replies in gb18030 by each of its names, in the header and in `overrideMimeType`.

## A request to `XMLHttpRequest` that the system refuses its thread

Compared with Qt 6.12.0 (PySide6). The system refuses the process every new thread (a soft
`RLIMIT_NPROC` of 0, `EAGAIN`) before `send()`, and nothing has been requested before.

| | Qt 6.12 | Here |
|---|---|---|
| An asynchronous request | logs `QThread::start: Thread creation error (Resource temporarily unavailable)`; `send()` returns, `readyState` stays 1, no handler runs, nothing is sent | `send()` returns with `readyState` 1; then `readyState` 4, `onerror`, `onloadend`, `status` 0, no text; nothing is sent; logs `XMLHttpRequest: cannot start the thread for a request:` and the reason |
| Threads are given again; the same request is opened and sent again | `readyState` stays 1, nothing is sent | goes out: states 1, 2, 3, 4, `onload`, `status` 200 |
| Threads are given again; a new request | `readyState` stays 1, nothing is sent | goes out, the same way |
| A synchronous request | logs the same line; `send()` has not returned 3 s later, nothing is sent | `send()` returns after `readyState` 4, `onerror`, `onloadend`, with `status` 0; nothing is sent; logs `XMLHttpRequest: cannot start the thread for a synchronous request:` and the reason |
| A synchronous request, an asynchronous one having run to its end before the refusal | the same as the row above: logs the line, `send()` has not returned 3 s later, the synchronous request is not sent (3 runs of 3) | the same as the row above: `send()` returns after `readyState` 4, `onerror`, `onloadend`, with `status` 0; logs the line |
| An asynchronous request, one having run to its end before the refusal | goes out: states 1, 2, 3, 4, `onload`, `status` 200; nothing is logged | the same |

After one refused asynchronous request Qt sends no other, also once threads are given
again (rows 2 and 3): measured for the same request opened and sent again and for a new
one, 3 s after which both have `readyState` 1, no handler has run and the server has seen
no connection. A synchronous request has a thread of its own in Qt and here, so a running
one does not take it (rows 4 and 5). The shell's own QML (`qml/`) sends no synchronous
request: it has no `XMLHttpRequest` at all.

Why: in Qt the request is never answered, and the QML that sent it waits for a handler that
does not run; a synchronous one stops the whole process. Here it ends the way a request
does that reached no server, in the same order (in both, for an address where nothing
listens: `send()` returns, then `readyState` 4, `onerror`, `onloadend`), so the QML's own
error handling runs, and the log says why.

Code: `run` and `send` in `crates/qml/src/xhr.rs`, on `workpool::try_long`
(`docs/threads.md`).

Tests, all in `crates/qml/tests/xhr_equiv.rs`, each run in Qt 6.12 and here:

- `a_request_refused_its_thread_fails_here_and_stays_opened_in_qt` checks the first three
  rows in both columns: Qt's process, waiting for a `loadend` of the two requests sent
  after threads were given again, is ended by an alarm 3 s later.
- `a_synchronous_request_refused_its_thread_fails_here_and_never_returns_in_qt` checks the
  fourth: Qt's process is ended by an alarm 3 s after the call, still inside `send()`.
- `a_synchronous_request_is_refused_its_thread_while_another_runs` checks the fifth, the
  same way.
- `a_request_goes_out_on_a_running_thread_while_new_ones_are_refused` checks the last row
  and the order of a request to an address where nothing listens.

## Log lines about a property declared under the name of a C++ property

A `property` declared in QML under the name of a property of the C++ base type hides that
property; under the name of a FINAL one the file does not load. Both are as in Qt 6.12.0
(`QQmlComponent`). Qt also writes lines to its log (category `qt.qml.propertyCache.append`)
which the engine does not write.

| QML | Qt 6.12 logs | Here |
|---|---|---|
| `Item { property real implicitWidth: 0 }` (also `implicitHeight`) | `qt.qml.propertyCache.append: Member implicitWidth of the object QQuickItem_QML_1 overrides a member of the base object. Consider renaming it or adding final or override specifier` | nothing |
| `Item { property int x: 1 }` (any FINAL name), before the error `Cannot override FINAL property` | `qt.qml.propertyCache.append: Final member x is overridden in class QQuickItem_QML_0. The override won't be used.` | nothing; the error is the same |
| `scale`, `state`, `spacing`, `currentIndex`, `resources` declared in QML | nothing | nothing |

Why: the lines are advice to the author of the QML and change nothing a script or the
screen can see. The class name in them (`QQuickItem_QML_1`) counts the QML types Qt has
made so far in the process, which the engine has no counterpart of.

`qmllint` of Qt 6.12 warns about the same declarations (`[property-override]`). The tree
has twelve of them in ten places:

| File and line | Declared | `qmllint` |
|---|---|---|
| `qml/modules/common/widgets/Box.qml:14` | `property alias spacing` in a `Grid` | `Property "spacing" already exists in base type "QQuickBasePositioner", use a different name.` |
| `qml/modules/common/widgets/CliphistImage.qml:42` | `property real scale` in a `Rectangle` | `Property "scale" already exists in base type "QQuickItem", use a different name.` |
| `qml/modules/common/widgets/StyledSwitch.qml:10` | `property real scale` in a `Switch` | the same |
| `qml/modules/rusty/overview/OverviewWidget.qml:28` | `property real scale` in an `Item` | the same |
| `qml/modules/rusty/overview/OverviewWindow.qml:18` | `property var scale` in an `Item` | the same |
| `qml/modules/common/widgets/StateLayer.qml:11` | `property var state` in a `Rectangle` | `Property "state" already exists in base type "QQuickItem", use a different name.` |
| `qml/modules/common/widgets/RippleButton.qml:160`, `:161` | `property real implicitWidth`, `implicitHeight` in an `Item` | `Member "implicitWidth" shadows member "implicitWidth" from base type "QQuickItem", use a different name or add a final or override specifier.` (and for `implicitHeight`) |
| `qml/modules/common/widgets/SecondaryTabButton.qml:118`, `:119` | the same two | the same two |
| `qml/modules/rusty/wallpaperSelector/WallpaperSelectorContent.qml:290` | `property int currentIndex` in a `GridView` | not for the file as it stands; for the declaration alone in a `GridView`: `Property "currentIndex" already exists in base type "QQuickItemView", use a different name.` |
| `qml/modules/rusty/overlay/resources/Resources.qml:18` | `property list<var> resources` in a `StyledOverlayWidget` | not for the file as it stands (`qmllint` does not resolve the base type); in an `Item`: `Property "resources" already exists in base type "QQuickItem", use a different name.` |

To remove the difference the engine has to write the two lines where it finds the hidden
name (`World::hides`, `World::check_final` in `crates/qml/src/runtime.rs`).

Test, in `crates/qml/tests/final_property.rs`:
`a_hidden_or_final_name_logs_nothing_besides_the_error`.

## `layer.textureSize` and `layer.mipmap` are not read

Qt renders an item with `layer.enabled: true` into a texture of `layer.textureSize` when it
is set, and with `layer.mipmap: true` gives that texture mipmap levels, which a layer shown
smaller than its texture is filtered from. The engine reads neither property: the texture
always has the item's size (times the screen's scale) and one level.

Pixels of a 160 x 96 scene that are not Qt 6.12's (OpenGL on llvmpipe), compared without
tolerance; in brackets, those further than 3 of 255 from Qt's in a channel:

| A 40 x 28 item with `layer.enabled: true` and | CPU | GPU |
|---|---|---|
| `layer.textureSize: Qt.size(10, 7)` | 248 (248) | 248 (248) |
| `layer.textureSize: Qt.size(10, 7); layer.smooth: true` | 543 (520) | 543 (520) |
| `scale: 0.25; layer.smooth: true; layer.mipmap: true` | 36 (36) | 26 (26) |

Why: the tree under `qml/` sets neither property, so nothing a user of the shell sees
depends on them.

To remove the difference the layer has to take its texture's size from `layer.textureSize`
and build the levels for `layer.mipmap` (the layer's texture in `crates/shell/src/scene.rs`,
`qsg::layer`, and the texture upload in `crates/shell/src/gpu.rs`).

Tests, in `crates/shell/tests/effects_qt.rs`: `layer_texture_size_is_not_read`,
`layer_texture_size_is_not_read_when_smooth`, `layer_mipmap_is_not_read` (`cpu` and `gpu`
of each). They hold the counts of the table: a change that brings a scene closer to Qt's or
takes it further fails them.

## A view in no window

Compared with Qt 6.12 (`QQuickView`, offscreen) on
`crates/shell/tests/views_like_qt/nw1.qml`: two `ListView`s 200×100 with 40 rows of height
20, one of them with sections, in a root `Item` that is in no window. After 200 ms the file
counts the rows that have a delegate.

| | Qt 6.12 | rusty-impulse |
|---|---|---|
| Delegates of the plain view | 6 | 22 |
| Delegates of the view with sections | 6 | 22 |
| `count` | 40 | 40 |

Why: Qt creates rows one at a time until their measured heights cover the view, and an item
in no window is never polished, so the view never tops the rows up to its `cacheBuffer`.
rusty-impulse sizes the range of rows from the last layout and does include the buffer. In a window
both make the same rows (`secw`, `sm7w`, `mm1w` in the same test file compare them line for
line).

Not changed, because the shell has no such view: its `ListView`s and `GridView`s are in the
content of panels and popups, and no file under `qml/services` has one. Changing it means
making rows one by one by measured height.

Test: `a_view_in_no_window_makes_more_rows_than_qt` in
`crates/shell/tests/views_like_qt.rs`.

## A view read in `Component.onCompleted`

Compared with Qt 6.12 on `crates/shell/tests/views_like_qt/oc1.qml`: a `ListView` 200×200
with 40 rows of height 30 and `highlightRangeMode: ListView.StrictlyEnforceRange` (range
60..90), in no window. Its own `Component.onCompleted` prints the view's geometry, and a
timer prints it again 100 ms later.

| Read in `Component.onCompleted` | Qt 6.12 | rusty-impulse |
|---|---|---|
| `contentY` | -60 | 0 |
| `contentHeight` | 1200 | 0 |
| `count`, `currentIndex`, `currentItem` | 40, 0, set | 40, 0, set |
| The same 100 ms later | -60, 1200 | -60, 1200 |

Why: Qt lays the view out in `QQuickItemView::componentComplete`, before the handler runs.
rusty-impulse lays a view out in the layout pass that follows the handlers.

Not changed, because nothing in `qml/` reads it there. Counted in `qml/`:

| | Places |
|---|---|
| `Component.onCompleted` handlers | 87 |
| of them reading `contentY`, `contentHeight`, `count`, `currentItem` or `currentIndex` of a view | 0 |
| of them calling `positionViewAtIndex`, `positionViewAtEnd` or `positionViewAtBeginning` | 0 |
| `positionViewAtIndex` calls, all outside `Component.onCompleted` | 2, both with `Contain`: `modules/rusty/wallpaperSelector/WallpaperSelectorContent.qml:308` (`GridView`), `modules/waffle/taskView/TaskViewContent.qml:257` (`ListView`) |
| `positionViewAtEnd` calls, all outside `Component.onCompleted` | 3: `modules/rusty/sidebarLeft/AssistantChat.qml:61` and `:194`, `modules/rusty/sidebarLeft/ScrollToBottomButton.qml:36` |

The one handler that touches a view, `modules/waffle/looks/WMenu.qml:72`, calls
`menuListView.itemAtIndex(0)?.forceActiveFocus()`: it needs the first delegate, which exists
by then in both, and reads no geometry.

Test: `a_view_in_no_window_is_laid_out_after_component_on_completed` in
`crates/shell/tests/views_like_qt.rs`.

## `GridView.flow: GridView.FlowTopToBottom`

Compared with Quickshell 0.3.2 (Qt 6.12) on `crates/shell/tests/views_like_qt/ftb1w.qml`: a
`GridView` 200×100 with 40 cells 50×40 and `flow: GridView.FlowTopToBottom`, in a
`FloatingWindow`.

| | Qt 6.12 | rusty-impulse |
|---|---|---|
| Cell 1 at | 0, 40 | 50, 0 |
| Cell 2 at | 50, 0 | 100, 0 |
| `contentWidth`, `contentHeight` | 1000, -1 | 0, 400 |
| `contentX`, `contentY` after `positionViewAtIndex(30, GridView.Beginning)` | 750, 0 | 0, 280 |

Why: Qt fills a column, then the next one, and scrolls along x. rusty-impulse lays a `GridView` out
in rows only and does not read `flow`.

Not changed, because no `GridView` in `qml/` sets `flow` (0 places).

Test: `a_grid_that_flows_top_to_bottom_is_laid_out_in_rows` in
`crates/shell/tests/views_like_qt.rs`.

## A number written to an enumeration that has an explicit C++ type

A number written to an enumeration property reads back as the enumeration's own C++ type.
For an enumeration without an explicit type the engine does what Qt 6.12 does
(`{horizontalAlignment: -1}` reads 4294967295). For one that has an explicit type (`quint8`,
`qint8`, `ushort`, `uint`: 22 of the 256 enumerations in `crates/qml/qmltypes`) Qt narrows
the number to that type and the engine does not. The key of an enumeration that another
class declares is refused as an initial property.

| QML (`Repeater`, whose `delegateModelAccess` is a `quint8` enumeration) | Qt 6.12 | Here |
|---|---|---|
| `createObject(root, {delegateModelAccess: -1})` | 255 | 4294967295 |
| `createObject(root, {delegateModelAccess: 256})` | 0 | 256 |
| `createObject(root, {delegateModelAccess: 257})` | 1 | 257 |
| `createObject(root, {delegateModelAccess: 4294967295})` | 255 | 4294967295 |
| `createObject(root, {delegateModelAccess: "ReadWrite"})` | 2 | no object, `Could not set initial property delegateModelAccess` |
| `createObject(root, {delegateModelAccess: 1})`, `255`, `1.7` | 1, 255, 2 | the same |

Why: 0 places. Nothing in `qml/` writes a number to such a property. The two properties of
this kind that `qml/` writes are `WlrLayershell.keyboardFocus` (16 places, each a key of
`WlrKeyboardFocus`) and `MprisPlayer.loopState` (one, in `MprisController.setLoopState`,
which nothing calls); neither is an initial property of `createObject`.

To remove the difference `types::EnumInfo` has to keep the `type` of the `.qmltypes` enum
and `valuetypes::qqml_property_write` (`WriteTarget::Enum`) has to narrow to it, and
`Eng::set_initial_property` has to find the keys of an enumeration by its qualified name
(`QQmlDelegateModel::DelegateModelAccess`).

Test, in `crates/qml/tests/audit_b3_env.rs`:
`create_object_does_not_narrow_an_enumeration_of_an_explicit_type` (the fixture
`audit_b3/create_object_enum_typed.cases` holds Qt's answers).

## `Component.onDestruction` of a singleton and of a root `Item` when the shell ends

Compared with `qml6` of Qt 6.12.0 and with Quickshell 0.3.2, at `Qt.quit()`.

| | Qt 6.12 (`qml6`) | Quickshell 0.3.2 | Here |
|---|---|---|---|
| Handlers of a `pragma Singleton` object and of the objects under it | Run last, after those of the document. | Do not run. | Do not run. |
| Handlers of a root `Item` and of the objects under it | Run. | Do not run. | Do not run. |

Why: the shell is run by Quickshell, not by `qml6`, so the engine ends the way Quickshell
does. Quickshell deletes the root object of the configuration and ends the process without
deleting the QML engine's singletons (`EngineGeneration::destroy`), and it puts a root `Item`
into a window of its own that it does not delete either. Every other handler runs in the
order of Qt 6.12.

Tests, in `crates/shell/tests/destruction_equiv.rs` (configurations and what Qt 6.12 and
Quickshell printed in `crates/shell/tests/destruction/`):

- `handlers_run_in_the_order_of_qt_except_those_of_singletons` (`order`) runs `qml6`,
  Quickshell and the native shell: `qml6` prints the two singleton lines last, the other two
  print the same lines without them.
- `a_root_quickshell_wraps_in_a_window_runs_no_handler` (`itemroot`) does the same for a
  root `Item`.

## `RadialGradient` and the unset properties of the Qt5Compat effects

Compared with Qt 6.12.0 (PySide6, and the `Qt5Compat.GraphicalEffects` of the image's Qt), a
fresh instance with nothing set.

| What | Qt 6.12 | Here |
|---|---|---|
| The name `RadialGradient` | The type of the last import that has it: the effect of `Qt5Compat.GraphicalEffects` or the gradient of `QtQuick.Shapes`. | The effect of `Qt5Compat.GraphicalEffects`, whatever the document imports; the gradient of `QtQuick.Shapes` is not written (`typecheck::NATIVE_OF`, the tree check names a document that asks for it). |
| The change signals of a property the effect's file binds, when the effect is created | Sent once the file's bindings are first evaluated, for a value that is not 0: `radiusChanged` and `deviationChanged` of a `GaussianBlur` (4 and 1.5), `radiusChanged` of a `DropShadow`, `horizontalRadiusChanged` and `verticalRadiusChanged` of a `RadialGradient` with a size. | Not sent: the property reads the value from the start. A later change of `samples`, `radius`, `width` or `height` sends them as in Qt. |
| What Qt's QML file of an effect is made of, 37 lines | `data`, `children` and `visibleChildren` count the file's items (`RadialGradient.children` has 4), and `GaussianBlur` has 19 properties whose names start with `_`. | The lists are empty and the `_` properties are `undefined`: an effect is drawn by the engine, there is no such file. |

Why: 0 places for each line. `qml/` creates `RadialGradient` in 2 places
(`qml/modules/common/widgets/RippleButton.qml:167`, `SecondaryTabButton.qml:126`), both in
files that import `Qt5Compat.GraphicalEffects` and not `QtQuick.Shapes`; none of the 6 files
that import `QtQuick.Shapes` writes the name. No file of `qml/` has a handler of
`radiusChanged`, `deviationChanged`, `horizontalRadiusChanged` or `verticalRadiusChanged`,
and a binding that reads such a property reads Qt's value. Nothing in `qml/` reads the
inside of an effect.

Tests: `unset_properties_read_what_qt_gives` in `crates/qml/tests/unset_defaults.rs` holds
the other 602 lines of `tests/unset_defaults/Qt5Compat.GraphicalEffects.txt`, the 37 are in
`tests/unset_defaults/leftover.txt` under their group from `owners.tsv`;
`crates/qml/tests/effect_bound_defaults.rs` holds what Qt prints for the bound properties,
less the signals of the second line; its
`an_effect_sends_the_signals_of_its_files_bindings_when_it_is_created` has all Qt prints and
the order of the signals (`h100 v40 gr4 gd1.5000 dr4`), is red and is `#[ignore]`d.

## `Text.horizontalAlignment` taken back under `LayoutMirroring`

Compared with `qml6` of Qt 6.12.0. A `Text` whose text reads left to right has
`horizontalAlignment: Text.AlignLeft`, `LayoutMirroring.enabled` is set, and then a script
takes the alignment back with `horizontalAlignment = undefined`.

| After `horizontalAlignment = undefined` | Qt 6.12 | rusty-impulse |
|---|---|---|
| `horizontalAlignment` read by a script | `Text.AlignLeft` | The same. |
| `effectiveHorizontalAlignment` read by a script | `Text.AlignLeft` | The same. |
| `horizontalAlignmentChanged` | Not sent. | The same. |
| `effectiveHorizontalAlignmentChanged` | Not sent, though the value read went from `AlignRight` to `AlignLeft`. | Sent. |
| A binding to `effectiveHorizontalAlignment` | Keeps `Text.AlignRight`, also after the text or the width changes. | `Text.AlignLeft`. |
| Where the text is drawn | At the right, where mirroring put it, until something else lays the text out. | At the left. |

Why: in Qt this is a state left behind, not a rule of layout. `QQuickText::resetHAlign`
(qtdeclarative 6.12, `src/quick/items/qquicktext.cpp`, line 2060) sets `hAlignImplicit`
and lays the text out only if `QQuickTextPrivate::determineHorizontalAlignment` returns
what `QQuickTextPrivate::setHAlign` (line 2087) returns for the alignment of the text's
direction. `setHAlign` compares the stored alignment, `AlignLeft` before and after, and returns false
without sending a signal; `effectiveHAlign` (line 2068) no longer mirrors an alignment
that is implicit, so the value it returns has changed and nobody was told. A `Text` that is
created mirrored with no alignment, or whose alignment is taken back before mirroring is
set, is at the left in Qt as well.

Everything else about the alignment of `Text` is as in Qt 6.12, taking an alignment back
included: text that reads right to left in the same steps, and either text without
mirroring.

The shell's `qml/` does not reach this: no file binds `horizontalAlignment` to `undefined` or
assigns it in a script (122 bindings in 80 files, each to an alignment), and none sets
`LayoutMirroring`.

Code: `horizontalAlignment` in `WorldHost::set` of `crates/qml/src/runtime.rs`.

Test: `an_alignment_taken_back_under_mirroring_is_laid_out_anew` in
`crates/shell/tests/text_align_qt.rs` runs the steps in Qt 6.12 and here, and fails if what
a script reads differs, if Qt stops keeping the state above, or if the signal, the binding
or the place of the text here is not as in this table.

## The drawn glyphs of `Text.QtRendering` are kept, 512 a font

Qt draws every glyph of such a text from its distance field each frame and keeps nothing drawn, so it has no limit. rusty-impulse keeps the drawn glyphs of a font at a size, 512 at most (`DF_TILES`): the 513th pushes out the one used longest ago, and that one is drawn anew with the same pixels when it is next asked for. The picture is Qt's; only the time differs. Test: `tile_past_the_limit_evicts_the_oldest_which_redraws_the_same` in `crates/qsg/src/text.rs`.

## `Text.QtRendering` under a `Rotation` about x or y, or under a `clip` inside a turned item

A `Text.QtRendering` under a `Rotation` about the x or y axis has not been compared with Qt, and one under a `clip` inside a rotated or scaled item is drawn into that item's layer and turned with it, where Qt draws its glyphs through the node's matrix. `qml/` has neither: 0 of its 7 such `Text`s and the `TabButton` label stand so.

## A `ShaderEffectSource` on screen

A `ShaderEffectSource` that stands on screen itself shows its `sourceItem` in Qt. In rusty-impulse it only feeds an effect and draws nothing: in a window 320 by 240 with white under the text, all 743 pixels of a line at 16 px that it shows in Qt are missing. `qml/` has none on screen.

An item with `layer.enabled: true` and no `layer.effect` is drawn as Qt draws it, into a transparent texture and that texture into the window. A `Text.QtRendering` in it has no opaque pixels under it there, so its glyphs come out gray, without the coloured fringes of subpixel antialiasing: a line at 16 px in a layer over both sides of the window and one at 20 px in a layer over the window's corner have Qt's pixels. So have a layer with a `layer.effect` and the source of an `OpacityMask` or a `GaussianBlur`.

Tests in `crates/shell/tests/text_qt_view.rs`, on the CPU and on the GPU: `a_shader_effect_source_on_screen_differs_from_qt_as_recorded` holds the number, and `a_shader_effect_source_on_screen_matches_qt` beside it is the comparison with no difference allowed, not run until the source is drawn (`--run-ignored all`). `text_in_a_layer_without_an_effect_matches_qt` compares the two layers with Qt, no difference allowed.

## A glyph of `Text.QtRendering` that an edge of the window cuts

Qt Quick's window has a depth buffer, so llvmpipe clips every triangle at the edges of the window before it draws it: a glyph's two triangles become up to seven, with new corners on the edge whose texture coordinates are computed there. rusty-impulse draws the two whole triangles and leaves out the pixels beyond the edge. The coordinates agree to the last bit or two of a `float`, which moves a pixel of the cut glyph by up to 4 of 255 now and then: of 30 glyphs at 16 px cut by the upper edge, 23 pixels differ, by up to 4; cut by the lower edge, 41 pixels, by up to 2; the one glyph cut by the left edge and the one cut by the right, none. A glyph cut by a `clip` or by the edge of a `Flickable` is not clipped so (Qt cuts it with the scissor) and is drawn as in Qt.

Tests: `text_cut_by_an_edge_of_the_window_differs_from_qt_as_recorded` in `crates/shell/tests/text_qt_view.rs` holds these numbers; `text_cut_by_an_edge_of_the_window_matches_qt` beside it is the comparison with no difference allowed, not run.

## Which moved item becomes a batch root

Qt's renderer makes an item that moves a batch root once more than 64 geometry nodes or more than 1024 vertices lie under it, and from then on places the glyphs of a `Text.QtRendering` under it from that item, which rounds differently in the last bit. rusty-impulse does the same and counts the nodes and vertices of a `Rectangle` and of a text as Qt has them (`text_moved_between_frames_matches_qt_on_both_sides_of_a_batch_root`: 256 and 257 glyphs, 64 and 65 nodes). Anything else that draws (an `Image`, a `Shape`, a `Canvas`, a `ShaderEffect`, a layer) it counts as one node of four vertices, which is not Qt's count for all of them: an item that holds such things and is within their true count of a limit may become a batch root a move earlier or later than in Qt. This has not been compared with Qt.

## Qt's scale variables

Compared with `qml6` of Qt 6.12.0 under sway with two outputs, at scale 1 and 1.5.
`QT_SCALE_FACTOR` and `QT_SCREEN_SCALE_FACTORS` do in rusty-impulse what they do in Qt
([env.md](env.md#qts-scale-variables)); the rows below are what differs. Quickshell 0.3.2
does with these variables what Qt does, so each row is a difference from Quickshell as
well; what Quickshell shows is in
[quickshell-differences.md](quickshell-differences.md#qts-scale-variables).

| | Qt 6.12 | rusty-impulse |
|---|---|---|
| `QT_SCALE_FACTOR` that is a number but not a finite one above 0; an infinite entry of `QT_SCREEN_SCALE_FACTORS` | Taken as the factor: no window appears, and nothing is logged about the variable. The values are in the table below. | The factor is 1, the entry is dropped, and one line in the log names the variable and the value. |
| `QT_FONT_DPI=N` | The whole interface is scaled by N/96 (192 doubles it), not only fonts; multiplied with `QT_SCALE_FACTOR`; a screen's entry of `QT_SCREEN_SCALE_FACTORS` replaces it on that screen. | Not read. |
| `QT_WAYLAND_FORCE_DPI=N` | The same as `QT_FONT_DPI=N`. | Not read. |
| `QT_WAYLAND_FORCE_DPI=physical`, `QT_USE_PHYSICAL_DPI=1` | The factor comes from the output's physical size (`QWaylandScreen::logicalDpi`, `QPlatformScreen::overrideDpi`). On headless outputs, which have no physical size: nothing, and 100/96 for `QT_USE_PHYSICAL_DPI`. Not checked on a monitor. | Not read. |
| `QT_ENABLE_HIGHDPI_SCALING=0` | The factor from the DPI above no longer scales the interface (with `QT_FONT_DPI=192` a 12pt font is 32 px instead); `QT_SCALE_FACTOR` and `QT_SCREEN_SCALE_FACTORS` still apply. | Not read. |
| `QT_SCALE_FACTOR_ROUNDING_POLICY=Round` (and the other policies) | Rounds the factor from the DPI and the entries of `QT_SCREEN_SCALE_FACTORS` (`1.3;1.6` is 1 and 2), not `QT_SCALE_FACTOR`. A window on the output at scale 1.5 then has a ratio of 4 with the entry 1.6, not 3. | Not read: the entries are taken as they are (Qt's default, `PassThrough`). |
| CPU rendering (`RI_RENDER=cpu`) on a compositor without `wp_viewporter` | Qt has no such renderer: a factor always enlarges the window. | The factor does not enlarge the window. |

The values no window can have. "Nothing logged" is: no line that a run without the
variable does not have.

| Value | Qt's factor | What `qml6` shows and logs | rusty-impulse |
|---|---|---|---|
| `QT_SCALE_FACTOR=0`, `-0` | 0 | A window of the size 2147483647. Nothing logged. | 1, one warning |
| `QT_SCALE_FACTOR=-1` | −1 | No window. Nothing logged. | 1, one warning |
| `QT_SCALE_FACTOR=nan`, `NaN` | NaN | No window. Qt logs `The cached device pixel ratio value was stale on window expose` and `… on window update`. | 1, one warning |
| `QT_SCALE_FACTOR=inf`, `+inf`, `INF`, `-inf` | ±Infinity | No window. Nothing logged. | 1, one warning |
| `QT_SCALE_FACTOR=` (empty), `abc`, `2,5`, `2x`, `1e999`, `infinity` | 1 | As without the variable. Nothing logged. | the same |
| `QT_SCREEN_SCALE_FACTORS=X;2` with X one of `0`, `-1`, empty, `abc`, `nan`, `-inf`, `1e999` | the entry is dropped: the first screen has 2, the second 1 | Nothing logged. | the same |
| `QT_SCREEN_SCALE_FACTORS=HEADLESS-1=X;HEADLESS-2=2` with X one of `0`, `-1`, `abc`, `nan` | the entry is dropped: `HEADLESS-1` has 1, `HEADLESS-2` 2 | Nothing logged. | the same |
| `QT_SCREEN_SCALE_FACTORS=inf;2`, `HEADLESS-1=inf;HEADLESS-2=2` | Infinity on the first screen, 2 on the second | No window on the first screen. Nothing logged. | the entry is dropped (`inf;2` then gives the first screen 2), one warning |

The warnings:

```
WARN: QT_SCALE_FACTOR=nan is not a scale factor a window can have: the factor is 1
WARN: QT_SCREEN_SCALE_FACTORS: the entry HEADLESS-1=inf is not a scale factor a window can have: the entry is dropped
```

Why: a shell without a window and without a word about the variable in its log is a
breakage, not a behaviour to repeat, so the shell keeps its windows and says what it did
not take. The five variables that are not read are not used by the original's configs; they are left
for a change of their own. The CPU renderer scales a window's picture through
`wp_viewporter` alone.

Code: `Factors::parse` in `crates/shell/src/hidpi.rs`.

Tests, in `crates/shell/tests/scale_factor_equiv.rs`, where Qt is the one inside Quickshell
0.3.2:

- `a_qt_scale_factor_no_window_can_have_is_one_and_one_warning` and
  `an_infinite_screen_factor_is_dropped_and_one_warning` hold the rows with a warning: Qt's
  factor and silence, the native shell's windows and its one line.
  `a_qt_scale_factor_that_is_no_number_is_one` and
  `a_screen_factor_that_is_no_number_above_zero_is_dropped` hold the rows where both are
  silent. `crates/shell/src/hidpi.rs` has the same values without a compositor.
- `the_other_scale_variables_are_not_read` holds the five variables: the native shell
  shows with each what it shows without, and Qt's factors for `QT_FONT_DPI=192`,
  `QT_WAYLAND_FORCE_DPI=192` and `QT_SCALE_FACTOR_ROUNDING_POLICY=Round`.
- What `qml6` itself shows, `QT_ENABLE_HIGHDPI_SCALING`, the physical DPI and the CPU
  renderer without `wp_viewporter` were measured or read in the code and are not held by a
  test.

## A layer past the texture size limit is said once in the log

Compared with Qt 6.12.0 (`qml6`, `QSGRhiLayer::setSize`) on a window of 160 x 64 with a
`GaussianBlur` and a `DropShadow` of 1e9 x 1e9 over one source of 40 x 40, both on Mesa's
software driver, whose limit is 16384 texels a side.

| | Qt 6.12 | rusty-impulse |
|---|---|---|
| Size the layer is drawn at | 16384 x 16384: each side bounded to the limit. | The same. |
| What is on screen | The source and nothing of the two effects. | The same pixels. |
| The log | 6 lines of level warning at every frame, one for each layer of the two effects: `QSGRhiLayer: Unsupported size requested: [1000000008, 1000000000]. Maximum texture size: 16384`. | 2 lines of level warn for as long as the process runs, one for the layer and one for the blur: `layer of 1000000008x1000000000 is past the texture size limit 16384: drawn at 16384x16384`. |

A line is said the first time a layer or a blur of that size is drawn. The same size asked
for again says nothing: not at the next frame, not when what is in the layer changes, and
not by another item that asks for a layer of the same size.

Why: the line names a mistake in the QML, and one line names it. Six lines at each frame
fill the log of a shell that runs for days.

Code: `say_clamped` in `crates/shell/src/gpu.rs`.

Tests:

- `layer_and_blur_past_the_texture_size_limit_are_said_once` in `crates/shell/src/gpu.rs`
  draws a layer and a blur past the limit for three frames, the third with other pixels in
  them, and fails if the log is anything but one line for the layer and one for the blur.

## A frame of an animation that changes no pixel

Compared with `qml6` of Qt 6.12.0 under sway, by the requests each sends its surface
(`WAYLAND_DEBUG`). While an animation runs Qt renders its window at every frame and sends
the whole buffer each time, whether a pixel changed or not. rusty-impulse sends a frame only when
it differs from the one shown.

| | Qt 6.12 | rusty-impulse |
|---|---|---|
| A step that changes no pixel (an item still moving after its opacity reached 0, a value that rounds to the same pixel) | The window is rendered and committed with a full buffer, and a frame callback is requested. | Nothing is sent: no commit, no buffer, no frame callback request. |
| What paces the steps, one window shown | The frame callback of each commit. | The frame callback after a step that sent a frame; a timer of one refresh interval after a step that sent none. |
| What paces the steps, more than one window shown (the original's bar and a popup) | A system timer of 13 ms at 60 Hz, apart from the frames (`QSGThreadedRenderLoop::startOrStopAnimationTimer`): more steps than frames. | The same as with one window: one step per refresh interval, 16.7 ms at 60 Hz. |

The numbers, three runs of Qt and five of rusty-impulse:

| | Qt 6.12 | rusty-impulse |
|---|---|---|
| One window, an item fades out in 100 ms and moves for 500 ms: commits with a buffer after it is gone | 24 | 0 after the frame that shows it gone |
| The same, commits without a buffer and frame callback requests after it is gone | 0 and 24 | 0 and 0 |
| The same, steps and time | 31 to 32, 514 to 543 ms | 31 to 32, 464 to 527 ms (590 and 675 ms once each, on a busy machine) |
| One window, the item stays visible: commits with a buffer | 32 | 30 |
| the original's notification popup under Hyprland, ten notifications 0.4 s apart: commits with a buffer, three runs | 60, 65, 65 (88 in a run before them) | 84, 63, 84 |
| The same, commits without a buffer | 0 | 0 |

What QML sees is the same but for the number of steps with more than one window: the
transition takes its duration, the properties end at their `to` values, `running`,
`started`, `stopped` and `finished` come in Qt's order, and `on<Property>Changed` runs at
every step. With more than one window shown Qt takes about 13 ms a step and rusty-impulse 16.7 ms,
so a handler that counted its calls would count fewer; nothing in `qml/` counts the steps
of an animated property or adds anything up in its change handler.

Held by `an_unchanged_frame_is_not_committed_and_the_transition_ends_on_time`,
`animation_frames_without_damage_go_by_the_vsync_timer` and
`an_animation_between_sent_and_unchanged_frames_ends_on_time` in
`crates/shell/tests/frame_callback_wait.rs`; Qt's side is
`crates/shell/tests/scenes/qt_frames/unchanged_tail.sh`.

## A frame that comes late while an animation runs

Compared with Quickshell 0.3.2 on Qt 6.12.0 and with `QQuickView` of Qt 6.12.0 (PySide6)
under sway at 60 Hz. A NumberAnimation of 300 ms; the handler of its second and of its third
step keeps the thread busy for 100 ms. The time from `start()` to `finished`, on the wall
clock, two runs each:

| | Qt 6.12, Quickshell | rusty-impulse |
|---|---|---|
| No busy handler | 292 to 310 ms | 317 ms |
| Two steps of 100 ms | 442 to 460 ms: some 145 ms more | 515 to 516 ms: 200 ms more |

Each late step costs rusty-impulse one refresh interval (16.7 ms) more than it costs Qt, and
the Timers that run with the animation are that much later too, as they count on its clock.

Why, from reading the code and not proven by an experiment: Qt's render thread asks the
main thread for the next step of the animations right after it took the frame over
(`QSGThreadedRenderLoop`, after the sync), so a step runs while the frame before it is
rendered and waits for the screen, and a step that took 100 ms is followed by the next
frame at once. rusty-impulse has one thread: it steps, draws, sends the frame and then waits for
the compositor's frame callback before the next step, so the frame after a step of 100 ms
waits for a callback once more.

Code: the main loop and `AnimClock` in `crates/shell/src/main.rs`.

Not held by a test that fails when it changes: the fixture is
`late_frames_make_an_animation_as_late_as_under_quickshell` in
`crates/shell/tests/timer_clock_equiv.rs`, ignored; it compares the two times and passes
when they are within 30 ms. What the Timers do once such an animation ended is the same as
in Qt and is held by the other tests of that file.

## A Repeater's model written while the Repeater builds

A handler that runs while a Repeater creates a delegate (the delegate's own
`Component.onCompleted`, or one of the delegate's `layer.effect`) and writes that Repeater's
model: in Qt 6.12 `QQuickRepeater::setModel` lets go of the delegates and creates the new
ones inside the write, before the handler's next statement. Here the build in progress
returns first, and the Repeater is built again after it. In both the delegate that was let
go stays readable to the handler that wrote, and the Repeater ends with the same delegates;
the order in which the handlers run differs.

Each delegate adds its `modelData` to a log, writes the model if it is the first one, and
adds its `modelData` in capitals; the result is the log and `count`:

| Delegate of `Repeater { model: root.m }`, `m: ["a"]` | Qt 6.12 | Here |
|---|---|---|
| `Component.onCompleted` of the delegate writes `root.m = ["b", "c"]` | `abBcCA 2` | `aAbBcC 2` |
| `Component.onCompleted` of the delegate's `layer.effect` writes `root.m = ["b"]` | `abBA 1` | `aAbB 1` |

Why: a write builds a Loader or a Repeater at once (`delegators_sync`), also from a handler
that another one's build runs, but not the one whose build is running
(`World::delegators_syncing`): that write is handled once the build returns. None of the 35
`layer.effect` components of the QML tree has a handler.

Code: `delegators_sync` and `Eng::update_delegators_inner` in `crates/qml/src/runtime.rs`.

Test, in `crates/qml/src/runtime.rs`:
`a_repeater_whose_model_is_written_during_its_build_rebuilds_after_it`.

## `eval` and `Function` of JavaScript text

| | Qt 6.12 | Here |
|---|---|---|
| `eval("1 + 1")` | `2` | the shell does not build |
| `new Function("a", "return a")`, `Function("return 1")` | a function | the shell does not build |
| `f.constructor("return 1")` for a function `f` held in a variable | a function | `EvalError` when the line runs |

Why: the installed shell runs JavaScript compiled to Rust when the shell is built. It has no
JavaScript parser and no evaluator, so text that arrives at run time has nothing to run it.

The build of the shell stops and names every such place in the QML tree:

```
ricompiled: eval or Function of JavaScript text (it has no compiled code): shell.qml#make, shell.qml#run
```

`Function` counts when it is called, taken as a value (`var F = Function`) or reached as
`.constructor` of a function written in place. The constructor of a function held in a
variable is not visible to the build; that call throws `EvalError` in the installed shell.
The engine built with the interpreter (the tests) runs all of these as Qt does.

Code: `uncompiled_eval` in `crates/qml/src/runtime.rs`, called by
`crates/ricompiled-gen/build.rs`; `reads_eval` in `crates/qml/src/ir.rs`.

Tests, in `crates/qml/src/runtime.rs`: `eval_sites_fail_the_compiled_build`,
`function_of_text_fails_the_compiled_build`,
`indirect_function_of_text_fails_the_compiled_build`. The first also holds that the QML tree
has no such place.

What the engine built without the interpreter throws, and that `eval(4)` (not text) still
gives `4`: `eval_of_text_throws_without_an_interpreter`,
`function_from_text_throws_without_an_interpreter` and
`function_constructor_of_a_value_throws_without_an_interpreter` in
`crates/qml/tests/no_interp.rs`.

## The status of an Image whose `source` is written

Compared with `qml6` of Qt 6.12.0: an `Image` created without a `source`, whose `source` a
function then sets to a file that is read without waiting (not `asynchronous`); the
function reads `status` and `implicitWidth` right after the write.

| | Qt 6.12 | rusty-impulse |
|---|---|---|
| `status` and the size of the picture right after `pic.source = "p20.png"` | `Image.Ready` and the size of the picture: the file is read inside the write, and the handlers of `progress`, the size and `status` run inside it. | In the shell, as Qt: the write loads the picture (`image_written` in `crates/qml/src/runtime.rs` has the shell measure the Image through its measure hook, `text_measure`). An engine without the measure hook has `Image.Null` and 0: the file is read by the next layout of the window (`measure_inner` in `crates/shell/src/scene.rs`), the handlers run then. |
| `widthChanged` and `implicitWidthChanged` of an item sized by its content | `widthChanged` first (`QQuickItem::setImplicitSize`). | `implicitWidthChanged` first: `measure_inner` writes the implicit size, then the size. No handler of an implicit size in `qml/` (0 of `onImplicitWidthChanged`, `onImplicitHeightChanged`). |

What a handler of `status` sees when the status does change is Qt's: the size of the
picture is set first (`the_size_of_a_picture_is_set_before_its_status_changes`).

A picture created with its `source` is not in the table: it has its status and its size
when it is completed, as in Qt (`crates/shell/tests/image_status_qt.rs`).

Where it shows in `qml/`: not counted.

The engine without the measure hook is held by
`a_source_written_after_creation_is_loaded_by_the_next_layout_not_by_the_write` in
`crates/shell/tests/image_status_order.rs`; the write that loads in the shell by
`crates/shell/tests/sync_load.rs`.

## The signal of a colour a style file binds, while its object is made

Compared with Qt 6.12.0 (PySide6, the Basic style), on a `TextField` and a `Label` with a
handler on each colour their style file binds to the palette (`color`,
`placeholderTextColor`, `selectionColor`, `selectedTextColor`; `color`, `linkColor`).

| | Qt 6.12 | The engine |
|---|---|---|
| Signals sent while the object is made | One for each binding whose colour is not the one the type starts with: `placeholderTextColorChanged` and `selectionColorChanged` of the `TextField`. | None. |

Why: `qml/` has 0 handlers of these signals on a `TextField`, `TextArea`, `Label` or
`ApplicationWindow`. Its one `onColorChanged`
(`modules/common/widgets/shapes/ShapeCanvas.qml:46`) is on a `Canvas` that declares its own
`property color color`.

Tests: `a_colour_bound_by_a_style_file_sends_no_signal_here_while_its_object_is_made` in
`crates/qml/tests/qt_differences.rs` counts the signals in Qt and in the engine.

## A binding that gives a symbol to a number property

Compared with Qt 6.12.0 (PySide6), on `property real p: v` and 13 number properties of Qt
types (`Item.z`, `Item.opacity`, `Rectangle.radius`, ...), when a script writes
`v = Symbol('s')`.

| | Qt 6.12 | The engine |
|---|---|---|
| The script's statement `v = Symbol('s')` | Throws `TypeError: Type error`: the binding of `p` runs inside the write, and the error of turning its symbol into a number is left for the script that wrote `v`. | Does not throw. The error is the binding's: it is logged with the binding's place. |
| `p` afterwards | The value it had. | The value it had. |

Why: `qml/` makes 0 symbols (`Symbol(` is in none of its files).

Tests: `wrong_type_writes` in `crates/qml/tests/wrong_type_writes.rs` holds the 14 lines in
`crates/qml/tests/wrong_type_writes/leftover.txt`; it fails when the engine gives another
answer for one of them.

## Six code points `QString::normalized` does not compose

Compared with Qt 6.12.0 and ICU 78.3 of the `tools/testenv` image, both on Unicode 17.0, as
the engine's data is (`icu_normalizer` 2.3). The code point alone, its canonical decomposition,
and the code point after `a` U+0323 and before U+0301, in the four forms, for every code point:
NFD and NFKD are the same in all three, NFC and NFKC differ from Qt for these six and no other.

| Code point | String | Qt 6.12, NFC and NFKC | Here and ICU 78.3 |
|---|---|---|---|
| U+105C9 | `105D2 307` | `105D2 307` | `105C9` |
| | `61 323 105C9 301` | `1EA1 105D2 307 301` | `1EA1 105C9 301` |
| U+105E4 | `105DA 307` | `105DA 307` | `105E4` |
| | `61 323 105E4 301` | `1EA1 105DA 307 301` | `1EA1 105E4 301` |
| U+16126 | `16126`, `1611E 1611E 1611F` | `16121 1611F` | `16126` |
| | `61 323 16126 301` | `1EA1 16121 1611F 301` | `1EA1 16126 301` |
| U+16127 | `16127`, `1611E 16129 1611F` | `16122 1611F` | `16127` |
| | `61 323 16127 301` | `1EA1 16122 1611F 301` | `1EA1 16127 301` |
| U+16128 | `16128`, `1611E 1611E 16120` | `16121 16120` | `16128` |
| | `61 323 16128 301` | `1EA1 16121 16120 301` | `1EA1 16128 301` |
| U+16D6A | `16D63 16D67 16D67` | `16D69 16D67` | `16D6A` |
| | `61 323 16D6A 301` | `1EA1 16D69 16D67 301` | `1EA1 16D6A 301` |

It shows in two places, both through `QString::normalized` in Qt:

- `String.prototype.normalize()`, with no argument, `"NFC"` or `"NFKC"`.
- The host of a url (UTS #46 normalizes it): `http://a\u{16126}b.com/` keeps U+16126 here and
  becomes `http://a\u{16121}\u{1611F}b.com/` in Qt, the same for U+16127 and U+16128. The
  other three do not reach a host in a form Qt leaves decomposed.

Why: this is not a rule of Unicode, it is two mistakes in `qtbase/src/corelib/text/qchar.cpp`
(v6.12.0). ICU on the same Unicode data composes all six, and so does every conformant
normalizer (UAX #15). Doing what Qt does would mean a normalizer of our own in place of
`icu_normalizer`.

- U+16126, U+16127, U+16128, U+16D6A compose in two steps from three code points outside the
  BMP (U+1611E U+1611E gives U+16121, and U+16121 U+1611F gives U+16126). `composeHelper`
  (lines 1934 to 1996) has stepped onto the low surrogate of the second code point when it
  composes the first two; it removes that code point and goes on from the same position,
  which is now the low surrogate of the third one. The third is read as a lone surrogate and
  is never tried against the new starter.
- U+105C9 and U+105E4 compose from a code point outside the BMP and U+0307 inside it.
  `ligatureHelper` (lines 1894 to 1931) looks a pair up only when both are on the same side
  of U+FFFF, and `uc_ligature_map` of `qunicodetables.cpp` has no entry for these two pairs
  (`createLigatureInfo` of `util/unicode/main.cpp` asserts that there is no such pair).

To remove the difference Qt has to compose them.

Code: `"normalize"` in `crates/qml/src/builtins.rs`, `normalize_ace` in
`crates/qml/src/qurlhost.rs`.

Tests: `six_code_points_compose_as_in_icu_where_qt_does_not` in `crates/qml/src/qurlhost.rs`
(the normalizer and the hosts) and `normalize_composes_as_icu_where_qt_does_not` in
`crates/shell/tests/normalize_nfc.rs` (JavaScript, interpreted and compiled), against
`crates/qml/tests/qt_unicode/qt.tsv` and `crates/shell/tests/scenes/normalize_nfc/scene.qt`,
which hold what Qt 6.12 and ICU 78.3 give.

## Two strings `localeCompare` places elsewhere

`String.prototype.localeCompare` is `QString::localeAwareCompare` in Qt (qv4stringobject.cpp),
which is ICU's `ucol_strcoll` in the locale of `LC_ALL`, `LC_COLLATE`, `LANG` and a comparison
of UTF-16 code units in the C locale (`QCollator::defaultCompare`, qcollator_icu.cpp of
v6.12.0). The engine sorts with `icu_collator` 2.3 on the same CLDR 48 data. Compared with
Qt 6.12.0 and ICU 78.3 of the `tools/testenv` image on every pair of 181 strings in 18
environments: the four that are the C locale agree on every pair, and in the fourteen ICU
sorts two strings stand elsewhere, the other 179 as in Qt.

| String | Qt | The engine |
| --- | --- | --- |
| `"\uD800"`, a surrogate with no pair | before U+FFFD, as a code point with no weight of its own | equal to U+FFFD |
| `"\uD840\uDC00"`, U+20000, an ideograph outside the BMP | between 一 and 三, by radical and strokes | after every ideograph of the BMP, by code point |

- ICU4X reads UTF-16 with errors replaced: `compare_utf16` gives a lone surrogate the weight
  of U+FFFD. Sorting it apart takes a collator of the engine's own, and such a string is not
  text.
- ICU4C is built with the root collation's Han order from Unihan (radical and strokes for
  every ideograph); the data `icu_collator` ships has the implicit order, which is by code
  point outside the URO.

Code: `locale_cmp` in `crates/qml/src/builtins.rs`.

Test: `every_pair_compares_as_in_qt` in `crates/shell/tests/locale_compare.rs`, against
`crates/shell/tests/scenes/locale_compare/scene.qt` (Qt) and `scene.ours` (the rows of the
engine where they differ; without the two strings they are Qt's).

## Four scripts HarfBuzz is told as they are

QTextEngine gives every HarfBuzz buffer Qt's own Unicode functions (`hb_qt_get_unicode_funcs`,
qharfbuzzng.cpp of v6.12.0): general category, combining class, mirrored character, script,
composition and decomposition. The engine gives HarfBuzz the same six on the data it already
itemizes with (`icu_properties` and `icu_normalizer` 2.3, Unicode 17), in place of HarfBuzz's
own tables, which are of the Unicode HarfBuzz was built with (18 in 15.1.0). Compared with
the functions Qt 6.12.0 hands over, on every code point and on every pair Qt composes:

| Function | HarfBuzz 15.1.0's tables differ from Qt in | of them assigned in Qt | The engine differs in |
| --- | --- | --- | --- |
| general category | 13 007 code points | 0 | 0 |
| combining class | 34 | 0 | 0 |
| mirrored character | 10 | 1 (U+221D) | 0 |
| script | 13 192 | 185 | 185 |
| decomposition | 11 | 11 | 0 |
| composition | 121 pairs | 121 | 0 |

The 13 007, 34 and 9 of the 10 are code points of Unicode 18, which Qt has not assigned.

The 185 are the four scripts Unicode 17 added. Qt's table from `QChar::Script` to HarfBuzz's
scripts (`_qtscript_to_hbscript`) lists them in another order than `QChar::Script` does, so a
character goes to HarfBuzz under the tag of its neighbour:

| Script | Code points | Qt tells HarfBuzz | The engine |
| --- | --- | --- | --- |
| Sidetic, U+10940..U+1095F | 26 | `Berf` | `Sidt` |
| Tolong Siki, U+11DB0..U+11DEF | 54 | `Tayo` | `Tols` |
| Beria Erfe, U+16EA0..U+16EDF | 50 | `Tols` | `Berf` |
| Tai Yo, U+1E6C0..U+1E6FF | 55 | `Sidt` | `Tayo` |

No font of the `tools/testenv` image and none the shell names has a glyph for any of them, so
no line is drawn otherwise: `fc-list ":charset=HEX" family` over the four blocks prints
nothing (U+05D0 the same way lists 19 families).

Composition and decomposition are Qt's, as lines drawn with the Unicode ones differ:
`_hb_qt_unicode_compose` takes the NFC of the pair (so "ᾀ" + U+0300 is the one glyph of "ᾂ"),
with the two faults of `QChar::composeHelper` above, and `_hb_qt_unicode_decompose` gives
U+FB2C as U+05E9 alone. That stays inside shaping; `String.prototype.normalize` is the
section above.

Code: `crates/qsg/src/hb_unicode.rs`, set on each buffer in `shape_run_clusters`
(`crates/qsg/src/text.rs`).

Tests: `hb_unicode::tests` in `crates/qsg/src/hb_unicode.rs` (each function on every code
point and pair against `crates/qsg/tests/qt_hb/qt.txt`, which `qt_hb/dump.sh` takes from Qt;
`every_code_point_has_the_script_qt_gives_harfbuzz_but_four_scripts` holds the 185), and
`composed_decomposed_and_mirrored_characters_match_qt` in
`crates/qsg/tests/hb_unicode_equiv.rs`: 35 614 lines in the 234 families of the image, glyph
by glyph and by width against Qt, 205 of them different before and none now.

The image has two Qt 6.12.0: the system one, on the system's HarfBuzz 15.1.0, as the engine
and Quickshell are, and the one of PySide6 the tests ask, with the HarfBuzz 14.5.0 Qt bundles.
The two HarfBuzz place a mark on a base a 1/64 px apart in some variable fonts (Rubik: 238 of
1 176 lines, Space Grotesk: 35 of 1 729); the engine is where the system Qt is, on all 48 552
lines of the image's families and eight of the shell's (Google Sans Flex among them).

## A whole double written to a string property

Compared with Qt 6.12.0 (PySide6), on `text` of `Text`, `TextInput`, `TextEdit` and `Label`,
on `objectName` and on a `property string` of a document, written by a script and through a
binding.

| Written | Qt 6.12 | The engine |
|---|---|---|
| `100000` (a whole number of the script) | `100000` | `100000` |
| `Math.round(100000.4)`, `200000 * 0.5`, a binding to a `property real` that is 100000 | `1e+05` | `100000` |
| `2.5`, `123456.789`, `12345678.5`, `0.0001`, `1e15`, `1e21`, `NaN`, `Infinity` | `2.5`, `123456.789`, `12345678.5`, `1e-04`, `1e+15`, `1e+21`, `nan`, `inf` | The same. |

Why: QV4 keeps a number as an int or as a double, and QString::number writes a double
with six significant digits or the shortest text that reads back, `1e+05` for 100000. The
engine has one kind of number (`Value::Num`), so a double that happens to be whole is written
by its digits, as an int is. The difference shows only for a double with a whole value from
100000 up whose digits end in zeros (100000, 2000000, 12000000); up to 99999 and from 1e15
both write the same text. A binding to `objectName` gives it the digits in Qt as well.

In the shell's QML there are 0 such writes: a bare number reaches a string property in
`SettingsSpinRow.qml` (`Math.round(root.value * 100) / 100`, the largest `to` of its users is
10000), as `Math.round(value * 100)` of a share between 0 and 1.5, and as whole counters
(`Notifications.unread`, `root.count`, `index + 1`); bytes and speeds are joined to a string
or go through `toFixed`.

Tests: `a_string_property_holds_what_qt_holds` in `crates/qml/tests/string_property_qt.rs`
holds the 22 lines by the text of both answers (`whole_doubles`); it fails when Qt or the
engine gives another answer for one of them.

## Theme icons

Compared with `qml6` of Qt 6.12.0 and Quickshell 0.3.2 under sway: ten items in one window,
`strace` on the files of the theme and Qt's own journal (`qt.gui.icon.loader`). Which
directory of a theme an icon comes from is Qt's (`crates/shell/tests/icon_theme_dirs.rs`),
and so is the order the `Kirigami.Icon`s of a window are asked in: the one that asked to be
polished last first (`QQuickWindowPrivate::polishItems`; `icons_are_polished_in_the_order_qt_polishes_them`
in `crates/shell/tests/kirigami_icon_size.rs`). The row below is what differs.

| | Qt 6.12 | rusty-impulse |
|---|---|---|
| The order the `Image { source: "image://icon/..." }` of a window are asked from the theme in | From the last one declared to the first (`QQmlObjectCreator::finalize` completes the last object created first, and `QQuickImageBase::componentComplete` loads). | From the first to the last, in the first layout of the window. |

Where it shows: the order of the `onStatusChanged` of those `Image`s, and which icons come
from an `icon-theme.cache` that is found damaged only when one icon is looked up (measured
with `Kirigami.Icon`, which are asked in Qt's order now:
`icons_asked_before_the_cache_is_found_damaged_are_the_ones_qt_asks_before_it`; not
measured with `Image`).

What is found for a name is kept with the name, as `QIcon::fromTheme` keeps the `QIcon` of
each of the 100 names asked last (`qtIconCache` in `qicon.cpp`): the same name at another
size or at another scale is picked among the files found for it the first time, whatever
became of the theme's cache since. Measured against one `qml6` whose output is given six
scales in turn, and with one name at two sizes
(`an_icon_asked_again_at_another_scale_comes_from_the_directories_found_for_it_first` and
`..._at_another_size_...` in `crates/shell/tests/icon_theme_dirs.rs`). The rows below
differ; none of them is measured.

| | Qt 6.12 | rusty-impulse |
|---|---|---|
| A name leaves the 100 while an item still shows it | The item's `QIcon` keeps its engine: another size of it is picked among the files found first. | The name is searched again the next time it is asked. |
| The icon theme is changed while the shell runs | Every icon is searched again (`QIconLoader::themeKey`). | A name found before the change stays as found; the themes read stay as read. |
| With plasma-integration (`KIconEngine`) | `KIconLoader`'s own caches. | Kept by name, size and scale. |

## Effects and antialiased edges: pixels one level off Qt's

Not on purpose: three scenes of `crates/shell/tests/effects_qt.rs` that do not have Qt 6.12's
pixels (llvmpipe, a 160 x 96 window). Every pixel counted is one level of one channel off
Qt's, and the counts are the same on every run. The tests compare without a tolerance, so
they fail; they are left out of the gate by `#[ignore]` until the pixels are Qt's
(`tools/ci/gate --run as-tester cargo nextest run -p shell --features shell/interp
--run-ignored only -E 'binary(effects_qt)'` shows them).

| Test (`::cpu`, `::gpu`) | QML | Pixels not Qt's, CPU / GPU |
|---|---|---|
| `a_blur_in_a_scaled_subtree_with_edges_on_pixel_centres` | `GaussianBlur { radius: 10; samples: 21 }` of 120 x 70 in an `Item` with `scale: 1.1`, its top and bottom edges on the centres of rows | 17 / 23, in columns 79 to 118 of rows 33 to 69 |
| `effects_of_a_fractional_size` | a `MultiEffect` with `contrast`, with `blur` and with a shadow, a `GaussianBlur` and a `DropShadow`, each 40.5 x 30.3 or 40.6 x 30.4 | 28 / 28, the same pixels on both |
| `subtrees_scaled_by_one_less_a_rounding_error` | rounded `Rectangle`s at places between pixels in an `Item` with `scale: 0.9999999999999999`, and in one with that scale and `rotation: 90` | 43 / 37: row 24 under the first rectangle's lower edge (12), rows 41 and 50 along the turned one's edges |

Where the shell's own QML (`qml/`) draws them:

- `a_blur_in_a_scaled_subtree_with_edges_on_pixel_centres`: not found. A `GaussianBlur` in a
  scaled subtree is the lock screen's wallpaper (`modules/rusty/background/Background.qml:205`,
  `modules/waffle/lock/WaffleLock.qml:67`, both `scale: 1.1`), and the two tests with its
  geometry have Qt's pixels. That no screen size puts it in this test's case is not shown.
- `effects_of_a_fractional_size`: not found at a screen scale of 1. No view of the shell that
  was counted has an effect of a fractional size other than the lock screen's blur and an
  `OpacityMask` over a tab's `Rectangle`, and both have Qt's pixels. A `MultiEffect` with
  `blur` is in `modules/rusty/bar/Workspaces.qml:87`, `modules/common/widgets/WaveVisualizer.qml:66`,
  `modules/common/widgets/StyledBlurEffect.qml` and
  `modules/rusty/screenTranslator/ScreenTextOverlay.qml:252`; one with a shadow is nowhere. At
  a screen scale of 1.5 or 2 a `MultiEffect` with `blur` of a whole size is one level off
  Qt's on GPU as well (`effects_dpr_qt.rs`, `multieffect_blur_dpr`: 82 pixels at 1.5, 241 at
  2), so the shell does show that.
- `subtrees_scaled_by_one_less_a_rounding_error`: the shell draws it, and it is not about the
  scale. A rounded `Rectangle` alone with an edge between pixels (`x: 12.8; y: 12.1; width:
  20.4; height: 12.2; radius: 4`) has the same 12 pixels of row 24 one level off Qt's on both
  paths (Qt 99, ours 98 in the red channel), and one of a whole size at `x: 12.5; y: 12.5`
  has 18 on CPU and 7 on GPU. Every rounded `Rectangle` whose edge is not on a pixel boundary
  is in this case: a size that follows a text's width at a scale of 1, most of them at 1.25
  and 1.5.

No cause is named for any of them yet, and none has been compared with Qt on a real graphics
card. The lock screen's own blur (a fractional size in a scaled subtree,
`a_blur_of_a_fractional_size_in_a_scaled_subtree` and `lock_screen_blur`) and a blur of a
fractional size (`a_blur_of_a_fractional_size`) have Qt's pixels on both paths.

One more scene of that file is one level off Qt's and is not left out: its test holds the
count as the most that may differ. `shadow_offset_scaled_coloured`, a `MultiEffect` with
`shadowBlur: 0.5`, `shadowScale: 1.15` and an offset: 28 / 28. No cause is named.

## A Kirigami.Icon's fade while the GUI thread is busy

Compared with Qt 6.12.0 and Kirigami 6.31.0 drawing with OpenGL on Wayland under sway, with
`QSG_FIXED_ANIMATION_STEP`. A `Kirigami.Icon` with `animated: true` fades from its old image
into the new one in rusty-impulse as in Qt: 200 ms on `InOutCubic`, at 0 in the frame of the write
and in the next one. What differs is the frame the fade begins to move in when the GUI
thread is kept longer than the output takes for a frame.

| | Qt 6.12 | rusty-impulse |
|---|---|---|
| A new `source` while the GUI thread is kept longer than a frame of the output (16.7 ms at 60 Hz) in every frame | Every frame is drawn, and the fade stays at 0 for up to 44 frames more before it moves. | The fade moves in the second frame after the write, as with a free thread. |

The frames Qt's fade stood at 0 past the two it always needs, with the GUI thread kept so
long in every step of the animations, on an idle machine; ten fades in a run, two runs for
each of the first four rows and 23 for the last:

| GUI thread kept | Qt 6.12 stood | rusty-impulse stood |
|---|---|---|
| 5 ms | 0 to 1 frames | 0 |
| 12 ms | 0 to 1 frames, 3 twice in one run | 0 |
| 20 ms | 3 to 27 frames | 0 |
| 30 ms | 0 to 26 frames | 0 |
| 45 ms | 1 to 44 frames | 0 |

With a free thread Qt stands 0 or 1 frames, and up to 18 with twelve runs at once on busy
processors.

Why Qt stands: the fade is a `QPropertyAnimation` of the icon (Kirigami `icon.cpp:58`,
started in `Icon::updatePolish`, `:367`), and Qt's animation driver moves it only after two
queued calls have run (`QAnimationTimer::startAnimations`, queued at
`qabstractanimation.cpp:662`, then `QUnifiedTimer::startTimers`, `:431`). A call queued
while queued calls run waits for the next round (`qcoreapplication.cpp:1844`, `:1884`), and
the update requests of the frames come through the same queue
(`qsgthreadedrenderloop.cpp:1717`, `qwaylandwindow.cpp:1776` and `:871`). Every update
request ahead of a queued call is one more frame drawn with the fade at 0, and a thread
kept past the frame callback leaves more than one of them in the queue.

Why rusty-impulse does not: it has no queue of deferred calls between the write and the animation
clock. The fade is a job of that clock from the frame of the write (`World::polish_icon` in
`crates/qml/src/runtime.rs`) and makes a step in every frame. How many frames Qt stands is
what its event queue happens to hold, not a rule of the fade.

Tests, in `crates/shell/tests/kirigami_icon_fade_equiv.rs`:

- `our_fade_moves_in_the_second_frame_after_the_write` holds the rusty-impulse column.
- `qt_kept_45_ms_in_every_step_stands_before_its_fades` holds the last row of the table: Qt
  kept 45 ms in every step stands 3 frames or more before one of its fades.
- `animated_icon_fades_into_its_new_source_like_qt` compares every frame Qt drew with ours
  by the time of Qt's fade, so it holds however long Qt stood.
- The rows for 5, 12, 20 and 30 ms were measured and are not held by a test.

## Models of delegates

The names a delegate has from its model (`index`, `model`, `modelData`, the roles), the
writes to them and the types of a ListModel's roles are Qt's: the cases `delegate_ctx_*.qml`,
`delegate_roles_listmodel.qml` and `list_model_*.qml` of `crates/rigen/check/cases/qml` carry
Qt 6.12's answers, for a ListModel, a number, an array, a list of objects, one object, a
FolderListModel and Quickshell's ScriptModel (its ObjectModel has the same one role and goes
the same way; QML cannot make one, so it has no case). The rows below are not Qt's. They are
not on purpose: they are what is left, written here so that nobody takes them for Qt's
behaviour.

| | Qt 6.12 | Here | In the QML tree |
|---|---|---|---|
| `ObjectModel { Item {} Item {} }` of QtQml.Models as a model | its items are the rows; `count`, `get(i)` | the document does not load, `Cannot assign to non-existent default property`: `ObjectModel` is Quickshell's type of the same name, a list of objects the shell fills, with no default property, no `count` and no `get` | not used |
| `DelegateModel` | a type of QtQml.Models | no such type | not used |
| `rows.push(x)` on the array a ListView or GridView was given, then `forceLayout()` | the view does not see it | the view reads the array again, once any property was written since it last looked | `forceLayout` is not called |
| `modelData` of a row of an array of JS objects | a QVariantMap: a new object at every read, a write to a member goes back to the delegate and tells who reads `modelData` | one copy the delegate keeps, written in place, telling nobody | no write to a member of `modelData`, no `===` on such a `modelData` |
| a JS object that is an element of a `property list<var>` | a QVariantMap: a new object at every read (`l[0] === l[0]` is false), a write to a member of one goes back to the list and tells who reads the list | the one object, written in place; who reads the list is told, and a ScriptModel that was given the object as a row takes a copy of it as the changed row, so its delegates get the new `modelData` as under Qt. An element nobody wrote is the row the model has, where Qt's model is given a new object for it | `toggleList[index].size = …` of the quick toggles in edit mode, which goes as under Qt; no `===` on such elements. `toggleList.splice(index, 1)` there: under Quickshell the buttons after the removed one in its row keep their looks and hold the types of the toggles that follow (the `removeBluetooth` shot of the scene `ptr_quickToggleEdit` of `tools/parity`), here each button stays the toggle it was |
| `Object.keys(model)` | the members of the model item's meta-object (`objectName,index,row,column,model,…`) | `objectName` and what any object lists | not used |
| `modelData = "3"` in the row `3` of a number | nothing is written, `modelData` stays the number | `modelData` is the string | `modelData` is not written |

Tests: `crates/qml/tests/delegate_differences.rs` holds each row but `DelegateModel` and the
element of a `list<var>`; `member_write_of_a_list_element_changes_its_row` of
`crates/qml/tests/script_model_rows.rs` holds what a ScriptModel's delegates see of such a
write, against Quickshell's own log.

## `paste()` called from a script while the clipboard's text is not in the process

`TextInput.paste()` and `TextEdit.paste()` (and the controls made of them) called from a
script. A paste key and a middle click are not part of this: no script runs between the key
and the text.

| | Qt 6.12 | rusty-impulse |
|---|---|---|
| The clipboard's text is in the process (the cases below) | Inserts inside the call. | The same: inserts inside the call. `copy(); paste(); field.text` reads the pasted text. |
| The clipboard's text is not in the process | Reads the selection from its owner inside the call: the interface thread waits until the owner has written the text and closed the pipe (`QWaylandDataOffer::retrieveData`). The script reads the pasted text right after the call. | `paste()` returns at once and has changed nothing: the script reads the old `text`, cursor and selection. The shell reads the owner's pipe from its event loop and inserts the text once the owner closed it. The change signals come then, in the order and with the values Qt gives them. |
| Several calls that wait | Each inserts before it returns. | They insert in call order. A call made while an earlier one still waits goes behind it, also when its text is in the process by then. |

The text is in the process when the script calls `paste()` in these cases:

1. The shell owns the clipboard: a script set `Rustshell.clipboardText`, or called `copy()`
   or `cut()` of an editor. The text is the one it set, also before the compositor has
   announced the selection (as `QWaylandClipboard::mimeData` answers from its own source).
   The source is on the Wayland connection of the shell's data device, as Qt's is: the
   compositor cancels it before it sends the offer of the selection that replaced it, and
   from that `cancelled` on the shell's text is not the clipboard's. The shell never reads
   the offer of its own source.
2. Another client owns the clipboard, a shell surface has the keyboard focus, and the shell
   has read the text of that selection to the end. The compositor gives the focused client
   the selection as an offer (`wl_data_device.selection`); the shell asks each offer for
   its text as soon as it comes and reads it from its event loop. `paste()` inserts that
   text. The offer, its text and the script come on one Wayland connection, in its order:
   from the moment a new offer has come until its text is read there is no text, never the
   text of the selection before.

It is not in the process, and the text comes after the script, in these cases:

1. Another client has just taken the clipboard and has not finished writing its text to
   the shell. This lasts as long as the owner takes to answer.
2. No shell surface has the keyboard focus. The compositor gives the selection only to the
   focused client, so Qt's clipboard is empty then; the shell asks the offer it holds, and
   without one inserts nothing.
3. `paste()` is called while a binding is being evaluated (from a function the binding
   calls). The engine delivers change signals after the binding, so the insert waits too.
4. An earlier `paste()` is still waiting (the third row above).

Why: reading inside the call means blocking the shell's only interface thread on another
program. A client that owns the clipboard and answers slowly, or not at all, would stop
the bar, the lock screen and every other window of the shell for that long. Qt accepts
this for one application; a shell does not.

`Rustshell.clipboardText` is not the text of case 2: it is read by a follower with a Wayland
connection of its own (data-control), which may hear of a new selection later or earlier
than the shell's data device does. `paste()` does not use it.

Nothing in `qml/` calls `paste()`: the shell's own paste button assigns
`Rustshell.clipboardText` (`modules/rusty/sidebarLeft/Translator.qml`).

Tests:

- `script_paste_inserts_in_the_call_with_the_text_and_after_the_script_without` in
  `crates/shell/tests/script_paste.rs`, on a real clipboard under a sway of its own: with
  the shell's own text and with a selection the shell has read, the script that called
  `paste()` returns the pasted text; while the owner of a new selection keeps the shell
  waiting, the same script returns the old text and the text comes afterwards, through one
  more read of the offer. It fails if the waiting case starts to insert inside the call,
  and if either of the other two stops doing so.
- `script_paste_inserts_the_text_of_the_data_device_s_offer`, in the same file: the shell
  talks to the compositor through a proxy that keeps the compositor's events from the
  follower's connection. After another client took the clipboard, `paste()` inserts the
  new text inside the call while `Rustshell.clipboardText` still reports the old one.
- `script_paste_inserts_the_text_of_the_selection_that_replaced_the_shells`, in the same
  file and through the same proxy: the shell owns the clipboard, another client takes it,
  and `paste()` inserts the new text, not the shell's.
- `script_paste_inserts_in_the_call_only_with_the_text_in_the_process` in
  `crates/shell/src/keys.rs`: the same two directions without a compositor, the order of
  calls that wait, and the call from a binding.
- `script_paste_matches_qt` and `script_paste_after_the_script_matches_qt` in
  `crates/shell/src/keys.rs`: 540 calls compared with Qt 6.12, once with the text inserted
  inside the call and read by the same script, once with the text inserted after the
  script. Both leave what Qt leaves and send the signals Qt sends.

## A method Qt declares that the engine has not written

Qt's `.qmltypes` declare every method of every type. The engine has written the ones `qml/`
calls, not all of them. The rest are listed in [`unwritten-methods.tsv`](unwritten-methods.tsv),
one row per declaring class and method.

| | Qt 6.12 | rusty-impulse |
|---|---|---|
| `typeof item.childAt` | `function` | `function` |
| `item.childAt(1, 1)` | The child, or `null`. | Throws `Error: Item.childAt() is declared by Qt and not written in this engine`, reported with the QML file and line like any error of a script. |
| A build of a tree that calls one (`ricompiled-gen`, `riaotcheck --strict`) | Builds. | Fails with `file:line: Item.childAt() is declared by Qt and not written in this engine`. |

Why: before this the call returned `undefined` (`null` on a singleton or the object of a
service) and the script went on with a wrong value. An error names the method to write.

The rule has no list of names. A method is written when a branch of the engine answers it;
`unwritten::list` asks the engine about every method of every type and writes the file.

What the file says about `qml/`:

| Column | Meaning |
|---|---|
| `types` | QML types that have the method. |
| `calls in qml/` | Calls on a receiver whose type the file shows: `m()`, `this.m()`, `id.m()`, `Singleton.m()`. The build fails on each. |
| `same name, receiver unknown` | Calls of that name on a value whose type only a run shows (`x.m()`). |
| `may be this method` | Those of them that can be this method: 0 unless the tree can hold an object of a type that has it (`unwritten::reach`). The test below fails on a row that is not 0. |

A singleton Qt deletes on `destroy()` (`StandardPaths.destroy()`, `Easing.destroy()`) is one
of the rows: here the call throws and the singleton stays.

Tests:

- `crates/qml/src/unwritten/scan.rs`, `the_list_in_docs_is_the_one_the_code_finds`: the file is
  what the code finds now, and no row may be reached from `qml/`.
  `UPDATE_UNWRITTEN_METHODS=1 cargo test -p qml --lib unwritten` rewrites the file.
- `crates/qml/tests/unwritten_methods.rs` runs `tests/unwritten/*.qml` in Qt 6.12 (and
  Quickshell) and here: a method is what Qt has, or its call says it is not written.

## `AnimatedImage` keeps one frame with `cache: true`

Compared with Qt 6.12.0 (`QQuickAnimatedImage`, `QMovie`) on a GIF of 940 bytes: 40 frames
on a canvas of 4000 x 4000, played for two rounds. Memory is the anonymous memory of the
process.

| | Qt 6.12 | rusty-impulse |
|---|---|---|
| `cache: true` (the default) | Keeps every frame it has shown (`QMovie::CacheAll`): 61 MB more with each frame of the first round, 2565 MB from then on. | Keeps the frame on screen and one canvas the next frame is drawn on: 131 MB at every frame. |
| `cache: false` | Keeps one frame (`QMovie::CacheNone`): 185 MB, 245 MB at the peak. | The same 131 MB. |
| A step back (`currentFrame` set to an earlier frame) with `cache: true` | Shows the kept frame. | Reads the file again from its first frame up to that one. |

What is on screen, `frameCount`, the delays, `currentFrame` and the signals are the same as
in Qt 6.12 with either value of `cache`: with `cache: true` any frame can be asked for by
hand, with `cache: false` only the next one or the first, as in `QMovie`. The difference is
the memory, and the time of a step back.

Why: a file of 940 bytes costs Qt 2.5 GB, and nothing on screen is different for it.

Code: `Movie` in `crates/shell/src/movie.rs`, `animated_texture` in `crates/shell/src/scene.rs`.

Tests:

- `memory_is_level_over_two_rounds` in `crates/shell/tests/animated_image_memory.rs` plays
  that file for two rounds with `cache` left alone and with `cache: false`, and fails if the
  memory after any frame is half a frame (30 MB) above the lowest.
- `current_frame_set_by_hand_shows_what_qt_shows` in
  `crates/shell/tests/animated_image_qt.rs` compares the pixels after steps forward, back and
  past the end with Qt 6.12, with `cache` on and off.

## `loops` ending a movie at its last frame with `cache: false`

Compared with Qt 6.12.0, on GIF and WebP files of three frames.

| | Qt 6.12 | rusty-impulse |
|---|---|---|
| `loops: 1` (any number from 0), `finishBehavior: AnimatedImage.FinishAtFinalFrame`, `cache: false` | The process dies of a segmentation fault (exit code 139) when the rounds end: endless recursion in `QQuickAnimatedImage::handleLoopCompletion`. | The movie stops at the last frame, `playing` goes false, `finished` comes: the signals Qt 6.12 gives for the same movie with `cache: true`. |

Why: there is nothing of Qt's to do the same as, and the QML asks for the last frame.

Code: `Player::handle_loop_completion` in `crates/qml/src/animated.rs`.

Tests, in `crates/shell/tests/animated_image_qt.rs`:

- `where_qt_dies_the_shell_stops_at_the_last_frame` runs Qt 6.12 on these cases and checks
  that it still dies of signal 11, and that the shell stops at the last frame with `playing`
  false and `finished`. If Qt stops dying, the test fails and the difference goes.
- `rounds_and_their_signals_are_qts` checks that in these cases the shell gives the signals
  Qt 6.12 gives with `cache: true`.

## A blurred `MultiEffect` shadow: a few pixels one level off

Not on purpose: what is left of a `MultiEffect` with `shadowEnabled` and its blur after it
was compared with Qt 6.12 on llvmpipe, on a page of `#e8e8e8`. The shadow's colour is Qt's:
a sharp shadow (`shadowBlur: 0`) of the same colours has Qt's pixels, and so have
`DropShadow` and `RectangularShadow`.

| Shadow of a 40 x 30 source, `shadowVerticalOffset: 6` | Pixels not Qt's, CPU / GPU | Furthest |
|---|---|---|
| `shadowColor: "#4d000000"` | 17 / 108 | one level |
| `shadowColor: Qt.rgba(0, 0, 0, 0.3)` | 25 / 108 | one level |

Held by `effect_shadow_of_a_whole_alpha` and `effect_shadow_of_a_fractional_alpha` in
`crates/shell/tests/effects_qt.rs`, which allow no more pixels than these. No cause is named
yet, and it has not been compared with Qt on a real graphics card. `qml/` draws no such
shadow: none of its `MultiEffect`s sets `shadowEnabled`.

## A zone's names where the machine's ICU is not 78.3

Qt's answers in the tree were taken with ICU 78.3, the one of the system the image of
`tools/testenv` has. Qt asks ICU for a zone's names (`t` and `tttt` of a date's format), and
Arch's Qt uses the system's ICU, so on a machine with another ICU the original under Quickshell names
some zones in other words than rusty-impulse does. rusty-impulse carries the names of ICU 78.3 in
`crates/qml/src/locale_data.rs` and asks no ICU of the machine.

| | Qt 6.12 with ICU 73 | Qt 6.12 with ICU 78.3, and here |
|---|---|---|
| `Qt.locale().toString(d, "tttt")` in `America/New_York`, `LANGUAGE=ja:de`, January | `アメリカ東部標準時` | `米国東部標準時` |
| `d.toLocaleString(Qt.locale("de_DE"), "t")` in `Antarctica/Troll`, January | `GMT` | `GMT+0` |
| `Qt.locale("kxv_IN").toString(d, "tttt")` in `Asia/Kolkata`, `LANG=` (ICU 73 has no data for `kxv`) | `GMT+05:30` | `bārat mānānka belā` |
| `Qt.locale("nds_DE").toString(d, "tttt")` in `Asia/Kolkata`, `LANG=` | `GMT+05:30` | `UTC+05.30` |

Of the answers the fixtures hold, those two versions of ICU differ in 227 of 7775
(`system_zone_names_qt`), 15 of 4900 (`locale_digits_qt`) and 0 of 58 (`zone_names_qt`).

Why: the shell runs on the PC's system Qt, and the names it shows there are the ones to
match. The Qt inside the PySide6 wheel of the image carries its own ICU 73, so a generator
of answers that depend on ICU's data or on Unicode's asks through
`tools/testenv/system_qt.py`, which puts PySide6 on the system's Qt libraries and ends with
a text when a Qt or ICU library of the process is not the system's or the system's ICU is
not 78.3. A file that imports PySide6 some other way fails
`every_place_that_asks_qt_asks_through_system_qt` unless that test names it: `OTHER` are
the places whose answers do not come of ICU (pixels, geometry, properties), `OWED` the ones
that do and still ask the wheel's Qt.

To move to another ICU: change `ICU` in `tools/testenv/system_qt.py`, run
`tools/gen/locale/gen.py` and the `qt.py` of `system_zone_names_qt`, `zone_names_qt` and
`locale_digits_qt` in the new image.

Tests, in `crates/qml/tests/`: `qt_answers_system_qt.rs`
(`the_qt_that_answers_has_the_systems_icu`,
`every_place_that_asks_qt_asks_through_system_qt`), `locale_data_generated.rs`,
`system_zone_names_qt.rs`, `locale_digits_qt.rs`, `zone_names_qt.rs`.

## The document of an asynchronous component and a handler that stays busy

Compared with Qt 6.12.0 (PySide6). A handler asks for a component with
`Qt.createComponent(url, Component.Asynchronous)`, connects to its `statusChanged`, then
calls `Qt.callLater` and starts a Timer with interval 0.

| The handler | Qt 6.12 | Here |
|---|---|---|
| Returns at once | the call, the Timer, then `statusChanged` | the same |
| Stays busy for 150 ms before the call and the Timer | `statusChanged`, the call, the Timer | the call, the Timer, then `statusChanged` |

Why: Qt reads the document in its loader thread, which posts "the document is here" when it
is done, and that can be while the handler still runs. Which comes first depends on how
long the read and the handler take. Here the event is posted when the handler that asked
has returned, so the order is the same every time: the one Qt has when the handler is
short. It is posted earlier in one case, as in Qt: when the handler loads another document
and waits for it (`Qt.createComponent` without `Component.Asynchronous`), the documents
asked before are read first.

Code: `World::post_loaded` in `crates/qml/src/runtime.rs`.

Tests, in `crates/qml/tests/posted_order.rs`:

- `the_document_of_a_component_arrives_after_the_handler_that_asked` runs the second row
  in Qt 6.12 and here and checks both columns.
- `a_call_and_a_timer_of_interval_0_run_in_the_order_asked` checks the first row, in the 24
  orders of the four requests.

## An asynchronous component on a loaded machine

`Qt.createComponent(file, Component.Asynchronous)` of a file the engine has not read. Qt 6.12
reads the file on the thread of its type loader, and what the handler that asked sees depends
on how far that thread is. Here the component is Loading with `progress` 0 until the handler
that asked returns, and its `progressChanged` and `statusChanged` come where the section above
says: what Qt answers in most runs.

Measured in the image of `tools/testenv` with the Qt script of
`crates/qml/tests/component_not_ready.rs`. One run at a time: 20 of 20 give the first answer.
200 runs, 24 at a time on 8 threads, measured twice:

| Fixture of `component_not_ready/` | Qt 6.12 in most runs, and here | Qt 6.12 in the other runs | Other runs of 200 |
|---|---|---|---|
| `NotReady.qml` | `loading status Loading progress 0`; every `createObject` and `incubateObject` gives null and "QQmlComponent: Component is not ready"; then `statusChanged` | `loading status Ready progress 1`; `createObject` and `incubateObject` make objects; no `statusChanged` comes | 19, 26 |
| `Args.qml` | `loading status 2 progress 0`; 90 messages, 22 of them "QQmlComponent: Component is not ready" | `loading status 1 progress 1`; 79 messages, 11 of them that one | 26, 17 |
| `Incubate.qml` | `fresh: Loading Loading Loading same false progress 0`, `missing: Loading progress 0`, then after `onCompleted ends`: `fresh1 a progressChanged 1 Loading`, the three `statusChanged Ready`, `missing progressChanged 1 Loading`, `missing statusChanged Error`, `callLater` | the seven below | 17, 6 |

The other answers of `Incubate.qml`, with the number of runs in the two measurements:

| What the run says | Runs |
|---|---|
| `fresh: Ready Ready Ready same false progress 1`, `incubate of loading: [object Object]`, `missing: Error progress 1`, and no `progressChanged` or `statusChanged` | 6, 3 |
| `fresh: Ready Ready Ready same false progress 1`, `missing: Loading progress 0`, and `callLater` before `missing progressChanged 1 Loading` | 0, 1 |
| `fresh: Ready Ready Loading same false progress 1` | 2, 0 |
| `fresh: Loading Ready Loading same false progress 0` | 1, 0 |
| `missing: Error progress 1`, the rest as in most runs | 2, 1 |
| `missing progressChanged 1 Loading` and `missing statusChanged Error` after the lines of the incubators | 3, 1 |
| `callLater` before `missing progressChanged 1 Loading` | 3, 0 |

A whole round of the tests also gave `loading status Loading progress 1` for `NotReady.qml`.

A run of Qt on a loaded machine may also hold a message of Qt's own that the fixture does not
ask for:

```
QBasicTimer::start: negative intervals aren't allowed; the interval will be set to 1ms.
```

Seen in `Incubate.qml`, whose messages were then "QQmlComponent: Component is not ready" and
this line, 2 against the 1 of most runs and of here: in 1 of 320 runs of the whole test file,
32 at a time; in 0 of 640 runs of Qt alone on each of the three fixtures. Here the line is
never printed. Of what was read in Qt, the one QBasicTimer the fixture's `Timer` reaches is
`pauseTimer` of `QUnifiedTimer::localRestart` (`qabstractanimation.cpp`), started with the
time the nearest pause has left.

Why: `QQmlComponentPrivate::loadUrl` (`qqmlcomponent.cpp`) asks the type loader for the file
(`getType` with `QQmlTypeLoader::Asynchronous`) and then reads `isCompleteOrError()`. If the
loader's thread is done by then, the component takes the type at once: Ready or Error,
`progress` 1, and nothing is left to tell. Otherwise it registers itself and reads
`progress()`, which the thread may have set to 1 in between; `status()` is Loading for as
long as it holds the type data, and the loader's answer is an event of the loop, standing
where the thread posted it among the ones the handler posts. Here no other thread reads the
file: the load is an event posted when the handler that asked has returned or waits for
another document (`World::post_loaded`), and `Runtime::component_load` ends it, in the order
of the `Qt.createComponent` calls.

The three tests run Qt again, up to 8 times, until its console holds the fixture's `// qt: `
lines one after another (the first column), and compare the whole output of that run with
the one run made here. A run of Qt that is not over in 30 s counts as one of the 8, and so
does one whose messages hold exactly the line of QBasicTimer above; if none holds the lines
the test fails. A message more or less in the run made here fails it as before. That run
has no shell directory: its documents have `file:` urls, as the ones of the Qt it is compared
with. `NotReady.qml` calls `Qt.quit()` itself when the component
is not Loading after its calls, which only a run of Qt with the second answer does: without
it that run waits for a `statusChanged` that does not come.

Tests, in `crates/qml/tests/component_not_ready.rs`:
`component_that_is_not_ready_warns_and_gives_null`,
`unsuitable_arguments_are_named_before_the_component_is_asked`,
`incubator_makes_its_object_on_a_later_turn`.

## A Material field that does not set `Material.containerStyle`

Not deliberate: left open because the QML tree has 0 such places, and no test holds it.

A `TextField` or `TextArea` in a file that imports `QtQuick.Controls.Material` is the
Material one in Qt 6.12 whether or not it sets `Material.containerStyle`: selection
`#e91e63` / `#ffffff`, height 56. Here it is the Material one only when
`Material.containerStyle` is set; without it the field is the one of the application's
style (Basic: selection `#0066ff` / `#090909`, height 40).

The two files of `qml/` that import `QtQuick.Controls.Material` and have a field,
`qml/modules/common/widgets/MaterialTextField.qml` and `MaterialTextArea.qml`, set
`Material.containerStyle`, `selectionColor` and `selectedTextColor`.

Test of the fields that do set it: `crates/shell/tests/editor_selection_equiv.rs`.

## Pixels of the scrolled assistant chat that are 1 of 255 from Qt's

Not on purpose, and no test holds it: found by comparing screenshots, left to be fixed apart
from the colours of the code block. Measured once, in a guest, against Quickshell 0.3.2 on
Qt 6.12.0, with no tolerance, in both themes (`Monokai` and `ayu Light`). A stub gives the
answer: text, a Rust code block and a Markdown code block. A second question in the same
chat then scrolls the first answer up by 205 px. The region is the chat, x 15–440,
y 121–959 of the screen. Here is `2b1fb9b1`: the code block's colours, with `5136207c`
(a tag over part of a right-to-left line, the stroke of an underline) merged in.

Every pixel counted differs by 1 in one channel; none differs by more.

| Where, after the second question | Dark | Light | The same build without the code block's colours |
|---|---|---|---|
| The edge of the "Code Block" chip that floats at the top of the scrolled chat (x 161–293, y 129–173): two pixels a row at its left and right, and a row along its lower edge (two rows in the light theme) | 125 | 273 | `main` (`7365e896`) and `5136207c` have the same values in 111 and 247 of them. |
| A row at the top edge of the chat, y 127, x 24–44 | 21 | 0 | The same values in 13. |
| A row under it, y 137, x 19–148: the background is (35,25,25), Quickshell's (35,25,24) | 130 | 0 | The same values in 115. |
| Corners of the message bubbles | 0 | 6 | `main` has the same 6. |
| The digit 3 in the column of line numbers of the three code blocks: (58,242), (59,247), (58,418), (59,423), (58,926) | 5 | 3 | `5136207c` has the same values in all 8 (those of the first Rust block 13 px lower). `main` draws these digits grey, without the coloured fringe, and differs from Quickshell over the whole digit. |
| All | 281 | 282 | |

The coordinates not counted in the last column (8, 15, 14 and 26) could not be compared:
without the code block's colours the first Rust block is 13 px shorter after the second
question (116 px for Quickshell's 129, its last line number cut), so the text above it stands
13 px lower and covers them.

Before the second question, the answer alone: the two code blocks have no pixel that
differs in either theme; the whole chat has 0 in the dark theme and 9 in the light one, all at
corners of the message bubbles, the same 9 on `main`.

Why: not known. Quickshell's own picture of the digit changes with the scroll: the pixel
that is (64,39,30) in the frame of the answer is (65,39,30) after the second question, where
rusty-impulse keeps (64,39,30).

Orange marks a pixel that differs, dark theme and light theme:

![The dark theme: 281 pixels](img/chat-second-question-dark.png)
![The light theme: 282 pixels](img/chat-second-question-light.png)

## `push` and `sort` of a frozen or non-extensible array with more than 65536 holes at its end

Compared with `qml6` of Qt 6.12. This difference comes from the merge of two fixes of rusty-impulse,
not from a choice about Qt: the array storage of audit slice B1 alone answers as Qt here, and
stopped when the tail of long arrays (B2-17) was merged over it.

| | Qt 6.12 | Here |
|---|---|---|
| `var a = [1]; a.length = 100000; Object.freeze(a); a.push(2)` | `100001`, and `a[100000]` is `2` | `TypeError: Type error` |
| `var a = [1, 2, 3]; a.length = 100000; Object.preventExtensions(a); a.push(4)` | `100001` | `TypeError: Type error` |
| `var a = [3, 1, 2]; a.length = 100000; Object.freeze(a); a.sort()` | sorts: `1,2,3` | `TypeError` |

Why: QV4 appends to the storage of an array whatever the array's integrity level is
(`SimpleArrayData::putArray`), and Qt 6.12 sorts such a frozen array in place. Here an array
whose `length` passes its last stored element by more than 65536 keeps those holes as a tail
without storage (`JsObject::tail` in `crates/qml/src/value.rs`), so that
`new Array(4294967295)` allocates nothing, and the methods that work on the storage do not
reach a tail: they take the path of ECMA-262, which refuses the write. An array with at most
65536 holes at its end does as Qt.

the original's tree freezes no array and writes no `length` (0 places in `qml/`).

Test: `array_storage_methods_of_a_long_frozen_array` in
`crates/qml/tests/builtins_b1_qt_equiv.rs`, fixture `longtail` with Qt's answers; the five
cases are listed in `KNOWN` there, so the test fails when one of them becomes equal to Qt.

## A hole of an array passed by `Function.prototype.apply`

Compared with PySide6 of Qt 6.12.0.

| | Qt 6.12 | Here |
|---|---|---|
| `Math.max.apply(null, [,,-5])` | `0` | `NaN` |
| `(function(a){ return typeof a + " " + String(a) + " " + (a === undefined) }).apply(null, [,1])` | `number NaN false` | `undefined undefined true` |
| `var a = [1,2]; a[70000] = 3; Math.max.apply(null, a.slice(69990))` | `3` | `NaN` |

Why: `apply` takes its arguments from the array with Get, so a hole is `undefined`
(ECMA-262 CreateListFromArrayLike); so do Qt's `Reflect.apply`, a spread call
(`Math.max(...[,,-5])`) and its `apply` of a sparse array, of an array-like object, and of
an array that was never filled (`new Array(3)`, `Array.from([,1])`, `[...[,1]]`). For an
array with plain storage Qt's `apply` copies the storage slots instead
(`FunctionPrototype::method_apply` in `qv4functionobject.cpp`), and the slot of a hole holds
V4's internal empty value, which then reaches the function as an argument. An array literal
with an elision, an array an element was deleted from, and what `slice`, `concat` and `map`
give for such an array all have such slots. No JavaScript value behaves like the empty
value: `typeof` says `number`, `String` gives `NaN`, `+ 1` gives `1`, a default parameter
is not used for it, and stored into an array it is a hole again.

Every function called this way gets it, a built-in one too. Measured on Qt 6.12 with a hole
as the argument (`f.apply(null, [,])` unless written out), each of them a line of the test:

| Function | Qt 6.12 | Here |
|---|---|---|
| `Math.max`, `Math.min`, `Math.abs`, `Math.round`, `Math.hypot`, `Number` | the hole counts as `0` | `NaN` |
| `String`, `String.prototype.concat` | `"NaN"` | `"undefined"` |
| `isNaN` / `isFinite` | `false` / `true` | `true` / `false` |
| `Array`, `Array.of`, `[].concat`, `[].push` with `[,1]` | the element stays a hole | the element is `undefined` |
| `parseInt`, `Boolean`, `String.fromCharCode`, `JSON.stringify`, `Number.isNaN`, `Array.isArray` | as for `undefined` | the same |

Other built-in functions were not measured. the original's code calls `apply` only on arrays `map`
built from arrays without holes.

To remove the difference a value has to be able to be V4's empty value, through every
operator and built-in function, in the interpreter and in the JavaScript compiled to Rust.

Code: `list_from_array_like` in `crates/qml/src/builtins.rs`.

Test: `expressions_give_what_qt_gives` in `crates/qml/tests/builtins_b2_qt_equiv.rs`. The
lines of `builtins_b2/cases.tsv` with a third field are these cases: each has to give
exactly that field here, and Qt 6.12 something else.

## A Repeater whose delegate is assigned while it runs (deferred)

Compared with Qt 6.12.0 (PySide6). A complete Repeater with a model of 2 is given
`delegate = null`, then its first delegate again, then another one.

| The assignment | Qt 6.12 | Here |
|---|---|---|
| `delegate = null` | Each row is removed with `index` already -1: `itemRemoved(0, item)`, its `Component.onDestruction`; `countChanged`, `delegateChanged`. | The same, but the removed delegates still read their old `index`. |
| A delegate after `null` | The rows are made (`itemAdded` each), `countChanged`, `delegateChanged`; then all are released last first and made again, `countChanged`. 4 objects for 2 rows. | The rows are made once: `countChanged`, `delegateChanged`, `countChanged`. 2 objects. |
| Another delegate | The rows are removed one by one as in the first row (`count` 0, `countChanged`), the new ones made, `countChanged`, `delegateChanged`; then released last first and made again. 4 objects. | The old rows are released last first with `count` still 2, the new ones made once, `delegateChanged`. 2 objects. |

Why it is so in Qt: QQuickRepeater::setDelegate gives the delegate to its
QQmlDelegateModel, whose setDelegate removes and inserts every row, which the Repeater
follows row by row (QQuickRepeater::modelUpdated); then it calls regenerate(), which clears
and makes all rows again.

Deferred, not fixed: nothing in `qml/` assigns a Repeater's delegate while it runs (0
places; the three Repeaters whose delegate is a property, in `ToolbarTabBar.qml`,
`WeekRow.qml` and `CalendarView.qml`, get it once when they are made). What the original would see
is the number of delegates made and the handlers of `itemAdded`, `itemRemoved` and
`countChanged` called fewer times.

Test: `repeater_given_a_delegate_at_run_time` in `crates/qml/tests/qt_leftovers.rs`
(`RepeaterDelegateBack.qml`), ignored until this is fixed; it compares the lines with Qt's
and is red.

## A bare name of a module imported only behind a qualifier

Not on purpose. A document that imports a module only as `import QtQuick as Q` has the
module's types as `Q.Item`, `Q.Timer`; Qt 6.12 knows no bare `Item` in it and rejects the
document when it loads. Here a bare name of a C++ type is that type whatever the document
imports, so the document loads.

| | Qt 6.12 | Here |
|---|---|---|
| `import QtQuick as Q` and `Q.Item { Item {} }` | the document does not load: `Bare.qml:3:2: Item is not a type` | loads, the inner object is an `Item` |

A name behind the qualifier is Qt's on both sides: `Q.Item` is the `Item` of that import, and
`Q.Nope {}`, also in the body of a `Component` nothing creates, is `Q.Nope is not a type`
(`crates/qml/tests/qualified_import.rs`, `qt_leftovers.rs` `unknown_type_error_has_the_column`).

Where the shell's own QML (`qml/`) has it: nowhere. Of its 624 documents 38 put a qualifier on
an import: on their own directories, on `.js` files and on `org.kde.kirigami`. Each of them
also imports `QtQuick` without one.

Test: `crates/qml/tests/qt_leftovers.rs`,
`bare_name_of_a_module_behind_a_qualifier_is_no_type`, with the fixture
`qt_leftovers/unknown/Bare.qml`. It asks for Qt's answer, so it fails; it is left out of the
gate by `#[ignore]` until the engine gives it (`tools/ci/gate --run as-tester cargo nextest
run -p qml --features interp --test qt_leftovers --run-ignored only` shows it).

## `originY` and `contentHeight` right after `positionViewAtIndex` on a row without a delegate

`positionViewAtIndex` on a row the ListView holds no delegate of moves the view where the
average row size puts that row and makes the rows in sight there inside the call, as Qt 6.12
does; `contentY` is Qt's when the call returns. Qt also lays the new rows out inside the call,
so `originY` and `contentHeight` read in the same handler are already those of the new rows.
Here the rows are laid out with the next layout of the scene: until then the handler reads the
`originY` and `contentHeight` of before the call. By the next frame both are Qt's.

The scene `crates/shell/tests/scenes/qt_view_content/position_unrealized_origin.qml` (20 rows
of 30 px, then 80 of 50; `positionViewAtIndex(80, ListView.Beginning)`):

| | Qt 6.12 | Here |
|---|---|---|
| read in the handler, after the call | `call y 2400 oy -1600 ch 5000` | `call y 2400 oy 0 ch 3000` |
| 100 ms later and at rest (`position_unrealized.qml`) | `y 2400 oy -1600 ch 5000` | the same |

In the QML tree: no handler reads `originY` or `contentHeight` after `positionViewAtIndex`.

Tests, in `crates/shell/tests/qt_view_content.rs`:
`position_unrealized_origin_differs_from_qt_as_recorded` holds the line read here;
`position_unrealized_origin` is the comparison with Qt's line, not run until the rows are laid
out inside the call (`--run-ignored all`); `position_unrealized` compares the rest with Qt.

# Filesystem steps of the fswatch scenario (crates/qml/tests/fswatch/watch.qml), one per line:
# `name<TAB>shell command` run with cwd = scenario base. Shared by record-qs.sh and the Rust test.
setup	mkdir -p d fl w2 && echo one > d/a && echo t > w2/target && ln -s ../w2/target d/link && echo x > fl/f1 && echo '{"v": 0}' > d/ad.json
write	printf two > d/a
append	echo more >> d/a
touch	touch d/a
atomic	printf three > d/.a.tmp && mv d/.a.tmp d/a
read	cat d/a >/dev/null
unrelated	echo z > d/b
delete	rm d/a
create	echo four > d/a
rename_away	mv d/a d/a.bak
write_old	echo five >> d/a.bak
recreate	echo six > d/a
chmod	chmod 600 d/a
link_write	echo t2 > w2/target
link_atomic	echo t3 > w2/.t && mv w2/.t w2/target
nodir_create	mkdir nd && echo x > nd/x
nodir_write	echo y > nd/x
trigger	touch d/trigger
trigger2	touch d/trigger
fl_create	echo yy > fl/f2
fl_modify	echo yyyy > fl/f1
fl_touch	touch -d 2001-01-01 fl/f1
fl_delete	rm fl/f2
fl_mkdir	mkdir fl/sub
fl_rename	mv fl/f1 fl/f0
fl_hidden	echo h > fl/.h
fl_subfile	echo s > fl/sub/s
xdir_atomic	printf seven > xd.tmp && mv xd.tmp d/a

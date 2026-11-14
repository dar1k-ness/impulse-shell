# Filesystem steps of the DesktopEntries watch scenario (crates/qml/tests/desktopwatch/apps.qml),
# one per line: `name<TAB>shell command` run with cwd = scenario base, XDG_DATA_HOME=base/home and
# XDG_DATA_DIRS=base/sys:base/late. Shared by record-qs.sh and the Rust test.
setup	mkdir -p home/applications/sub/deep sys/applications && printf '[Desktop Entry]\nType=Application\nExec=x\nName=A\n' > home/applications/a.desktop && printf '[Desktop Entry]\nType=Application\nExec=x\nName=B\n' > sys/applications/b.desktop && printf '[Desktop Entry]\nType=Application\nExec=x\nName=M\n' > sys/applications/m.desktop && printf '[Desktop Entry]\nType=Application\nExec=x\nName=C\n' > home/applications/sub/c.desktop
new	printf '[Desktop Entry]\nType=Application\nExec=x\nName=N\n' > home/applications/n.desktop
inplace	printf '[Desktop Entry]\nType=Application\nExec=x\nName=N2\n' > home/applications/n.desktop
atomic	printf '[Desktop Entry]\nType=Application\nExec=x\nName=N3\n' > home/applications/.n.tmp && mv home/applications/.n.tmp home/applications/n.desktop
sub_new	printf '[Desktop Entry]\nType=Application\nExec=x\nName=D\n' > home/applications/sub/d.desktop
deep_new	printf '[Desktop Entry]\nType=Application\nExec=x\nName=E\n' > home/applications/sub/deep/e.desktop
sys_delete	rm sys/applications/b.desktop
mask	printf '[Desktop Entry]\nType=Application\nHidden=true\n' > home/applications/m.desktop
deep_new2	printf '[Desktop Entry]\nType=Application\nExec=x\nName=F\n' > home/applications/sub/deep/f.desktop
parent_touch	touch home/x
nodisplay	printf '[Desktop Entry]\nType=Application\nExec=x\nName=H\nNoDisplay=true\n' > home/applications/h.desktop
new_subdir	mkdir home/applications/sub2
sub2_new	printf '[Desktop Entry]\nType=Application\nExec=x\nName=G\n' > home/applications/sub2/g.desktop
late_mkdir	mkdir -p late/applications
late_new	printf '[Desktop Entry]\nType=Application\nExec=x\nName=L\n' > late/applications/l.desktop
sub_touch	touch home/applications/sub/c.desktop
rm_dir	rm -r home/applications/sub
